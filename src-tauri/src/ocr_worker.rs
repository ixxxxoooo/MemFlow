// @author ygw
//! OCR 子进程:把 Vision 识别放到独立进程里执行。
//!
//! 背景:macOS 首次使用某个可执行文件做 Vision 识别时,系统要编译神经引擎模型(约 20 秒),
//! 之后**同一进程内**的所有识别都会报 `e5rt` 错误,直到进程重启才恢复(通常再经过一到两次
//! 重启缓存才完整)。扫描版 PDF 有很多页,不能让第 2 页起全部失败,所以:
//!
//! - 主进程启动自身的一个子进程(参数 `--ocr-worker`)常驻,通过 stdin/stdout 按行通信;
//! - 某次识别失败就杀掉子进程、换新进程重试,最多 `MAX_ATTEMPTS` 次;
//! - 子进程空闲一段时间自动退出,释放内存;
//! - 子进程无法启动时,调用方退回进程内直接识别。

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::Mutex;
use std::time::Duration;

/// 子进程命令行标志
pub const WORKER_FLAG: &str = "--ocr-worker";
/// 单页识别最长等待(含首次编译神经引擎模型)
const REPLY_TIMEOUT: Duration = Duration::from_secs(150);
/// 单页最多尝试次数(每次失败都换新进程)
const MAX_ATTEMPTS: usize = 5;
/// 子进程空闲多久后自行退出
const IDLE_EXIT: Duration = Duration::from_secs(90);

/// 子进程识别结果
pub enum WorkerResult {
    /// 识别成功,附 JSON 文本
    Json(String),
    /// 子进程可用,但多次重试后仍失败
    Failed,
    /// 无法启动子进程(调用方应退回进程内识别)
    Unavailable,
}

/// 子进程的一行回复
enum Reply {
    /// 识别成功
    Ok(String),
    /// 子进程报告识别失败
    Err,
    /// 子进程已退出(EOF)
    Eof,
}

/// 与子进程的连接
struct Conn {
    child: Child,
    stdin: ChildStdin,
    replies: Receiver<Reply>,
    /// 这个子进程是否回复过任何一行(用于区分"协议不可用"与"识别失败")
    replied: bool,
}

impl Conn {
    fn kill(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

static CONN: Mutex<Option<Conn>> = Mutex::new(None);

/// 子进程可执行文件:`MEMFLOW_OCR_EXE` 优先(集成测试用),否则是当前程序自身
fn worker_exe() -> Option<std::path::PathBuf> {
    std::env::var_os("MEMFLOW_OCR_EXE")
        .map(Into::into)
        .or_else(|| std::env::current_exe().ok())
}

fn spawn() -> Option<Conn> {
    let exe = worker_exe()?;
    let mut child = Command::new(exe)
        .arg(WORKER_FLAG)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let stdin = child.stdin.take()?;
    let stdout = child.stdout.take()?;
    let (tx, rx) = mpsc::channel::<Reply>();
    // 读线程:一行一个回复;EOF(子进程退出)时发送 None 后结束
    std::thread::spawn(move || {
        let mut r = BufReader::new(stdout);
        loop {
            let mut line = String::new();
            match r.read_line(&mut line) {
                Ok(0) | Err(_) => {
                    let _ = tx.send(Reply::Eof);
                    return;
                }
                Ok(_) => {
                    let line = line.trim_end_matches(['\n', '\r']);
                    let reply = match line.strip_prefix("OK ") {
                        Some(j) => Reply::Ok(j.to_string()),
                        None => Reply::Err,
                    };
                    if tx.send(reply).is_err() {
                        return;
                    }
                }
            }
        }
    });
    Some(Conn {
        child,
        stdin,
        replies: rx,
        replied: false,
    })
}

/// 请求子进程识别一页
///
/// 参数: path PDF 路径;idx 页下标
/// 返回: 识别结果,见 `WorkerResult`
pub fn ocr_via_worker(path: &Path, idx: usize) -> WorkerResult {
    // 路径含制表符/换行的极端情况不走子进程
    let p = path.to_string_lossy().into_owned();
    if p.contains(['\t', '\n', '\r']) {
        return WorkerResult::Unavailable;
    }
    let mut guard = CONN.lock().unwrap_or_else(|e| e.into_inner());
    for attempt in 0..MAX_ATTEMPTS {
        if guard.is_none() {
            match spawn() {
                Some(c) => *guard = Some(c),
                None => return WorkerResult::Unavailable,
            }
        }
        let conn = guard.as_mut().expect("刚刚创建");
        let sent = writeln!(conn.stdin, "{idx}\t{p}").and_then(|_| conn.stdin.flush());
        let reply = if sent.is_ok() {
            conn.replies
                .recv_timeout(REPLY_TIMEOUT)
                .unwrap_or(Reply::Eof)
        } else {
            Reply::Eof
        };
        match reply {
            Reply::Ok(json) => {
                conn.replied = true;
                return WorkerResult::Json(json);
            }
            Reply::Err => conn.replied = true,
            Reply::Eof => {}
        }
        let never_spoke = !conn.replied;
        tracing::warn!("OCR 子进程第 {} 次识别失败,换新进程重试", attempt + 1);
        if let Some(c) = guard.take() {
            c.kill();
        }
        // 新进程一句话都没回就退出:说明这个可执行文件不支持子进程协议(如测试程序),
        // 不再反复重试,让调用方退回进程内识别
        if never_spoke && attempt == 0 {
            return WorkerResult::Unavailable;
        }
    }
    WorkerResult::Failed
}

/// 若本进程是被拉起的 OCR 子进程,则运行识别循环并返回 true(调用方应直接退出)。
///
/// 协议:stdin 每行 `页下标<TAB>PDF 路径`;stdout 每行 `OK <json>` 或 `ERR`。
pub fn maybe_run_worker() -> bool {
    if std::env::args().nth(1).as_deref() != Some(WORKER_FLAG) {
        return false;
    }
    run_worker();
    true
}

#[cfg(target_os = "macos")]
fn run_worker() {
    use crate::pdf_native::PdfDoc;
    // 读 stdin 放到线程里,主循环才能做空闲超时
    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        for line in BufReader::new(std::io::stdin())
            .lines()
            .map_while(Result::ok)
        {
            if tx.send(line).is_err() {
                return;
            }
        }
    });
    let mut cached: Option<(String, PdfDoc)> = None;
    let stdout = std::io::stdout();
    loop {
        let line = match rx.recv_timeout(IDLE_EXIT) {
            Ok(l) => l,
            Err(_) => return, // 空闲超时或父进程关闭了 stdin
        };
        let mut parts = line.splitn(2, '\t');
        let idx = parts.next().and_then(|x| x.parse::<usize>().ok());
        let path = parts.next().map(str::to_string);
        let reply = match (idx, path) {
            (Some(idx), Some(path)) => {
                if cached.as_ref().map(|(p, _)| p != &path).unwrap_or(true) {
                    cached = PdfDoc::open(Path::new(&path)).map(|d| (path.clone(), d));
                }
                cached
                    .as_ref()
                    .and_then(|(_, d)| d.ocr_json_direct(idx))
                    .map(|j| format!("OK {}", j.replace(['\n', '\r'], " ")))
                    .unwrap_or_else(|| "ERR".to_string())
            }
            _ => "ERR".to_string(),
        };
        let mut out = stdout.lock();
        if writeln!(out, "{reply}").and_then(|_| out.flush()).is_err() {
            return;
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn run_worker() {}
