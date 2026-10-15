// @author ygw
//! 文件夹递归扫描:为「导入文件夹」收集可导入的文件,并把被跳过的内容按原因汇总。
//!
//! 规则:
//! - 跳过隐藏文件/目录(以 `.` 开头)、`node_modules`、`__pycache__` 等噪音目录
//! - 不跟随符号链接(避免循环与越界访问),符号链接按"跳过"计数
//! - 目录深度 ≤ 8、文件数 ≤ 2000;超出部分忽略并提示
//! - 只收集受支持的文件类型,其余按扩展名汇总为"不支持的类型"

use crate::extract::{classify, ext_lower, FileKind};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// 最大递归深度(根目录为第 0 层)
pub const MAX_DEPTH: usize = 8;
/// 单次导入的文件数量上限
pub const MAX_FILES: usize = 2000;

/// 不进入的目录名(小写比较)
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "__pycache__",
    "target",
    "dist",
    "build",
    "venv",
];

/// 一个待导入的文件
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderFile {
    /// 绝对路径
    pub path: PathBuf,
    /// 相对导入根目录的路径(含根目录名,统一用 `/` 分隔),用于记录来源
    pub rel: String,
}

/// 扫描结果
#[derive(Debug, Default)]
pub struct Scan {
    /// 可导入的文件(按路径排序,结果稳定)
    pub files: Vec<FolderFile>,
    /// 被跳过的内容:原因 → 数量
    pub skipped: BTreeMap<String, usize>,
}

impl Scan {
    /// 记一笔跳过
    fn skip(&mut self, reason: impl Into<String>) {
        *self.skipped.entry(reason.into()).or_insert(0) += 1;
    }

    /// 跳过总数
    pub fn skipped_total(&self) -> usize {
        self.skipped.values().sum()
    }
}

/// 递归扫描文件夹
///
/// 参数: root 导入的根目录
/// 返回: 可导入文件与被跳过内容汇总
pub fn scan(root: &Path) -> Scan {
    let mut out = Scan::default();
    let root_name = root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("文件夹")
        .to_string();
    walk(root, &root_name, 0, &mut out);
    out.files.sort_by(|a, b| a.rel.cmp(&b.rel));
    out
}

/// 递归遍历一个目录
fn walk(dir: &Path, rel: &str, depth: usize, out: &mut Scan) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        out.skip("无法读取的目录");
        return;
    };
    let mut entries: Vec<_> = rd.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        let path = e.path();
        // 不跟随符号链接:symlink_metadata 看到的是链接本身
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            out.skip("无法读取的文件");
            continue;
        };
        if name.starts_with('.') {
            out.skip("隐藏文件/目录");
            continue;
        }
        if meta.file_type().is_symlink() {
            out.skip("符号链接");
            continue;
        }
        let child_rel = format!("{rel}/{name}");
        if meta.is_dir() {
            if SKIP_DIRS.contains(&name.to_lowercase().as_str()) {
                out.skip(format!("忽略的目录({name})"));
            } else if depth + 1 > MAX_DEPTH {
                out.skip(format!("目录层级超过 {MAX_DEPTH} 层"));
            } else {
                walk(&path, &child_rel, depth + 1, out);
            }
        } else if meta.is_file() {
            if classify(&path) == FileKind::Unsupported {
                let ext = ext_lower(&path);
                out.skip(if ext.is_empty() {
                    "不支持的类型(无扩展名)".to_string()
                } else {
                    format!("不支持的类型(.{ext})")
                });
            } else if meta.len() == 0 {
                out.skip("空文件");
            } else if out.files.len() >= MAX_FILES {
                out.skip(format!("超过 {MAX_FILES} 个文件的上限"));
            } else {
                out.files.push(FolderFile {
                    path,
                    rel: child_rel,
                });
            }
        }
    }
}

/// 把跳过汇总渲染为人类可读的行,如 `不支持的类型(.zip) ×3`
pub fn skip_lines(skipped: &BTreeMap<String, usize>) -> Vec<String> {
    skipped.iter().map(|(r, n)| format!("{r} ×{n}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn touch(p: &Path, content: &str) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, content).unwrap();
    }

    #[test]
    fn 收集受支持文件并记录相对路径() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("手册");
        touch(&root.join("a.md"), "# a");
        touch(&root.join("子/b.txt"), "b");
        touch(&root.join("子/孙/c.png"), "png");
        let s = scan(&root);
        let rels: Vec<&str> = s.files.iter().map(|f| f.rel.as_str()).collect();
        assert_eq!(rels, vec!["手册/a.md", "手册/子/b.txt", "手册/子/孙/c.png"]);
        assert_eq!(s.skipped_total(), 0);
    }

    #[test]
    fn 跳过隐藏噪音目录空文件与不支持类型并汇总原因() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("r");
        touch(&root.join("ok.md"), "x");
        touch(&root.join(".git/config"), "x");
        touch(&root.join(".DS_Store"), "x");
        touch(&root.join("node_modules/pkg/index.md"), "x");
        touch(&root.join("empty.md"), "");
        touch(&root.join("a.zip"), "z");
        touch(&root.join("b.zip"), "z");
        touch(&root.join("noext"), "z");
        let s = scan(&root);
        assert_eq!(s.files.len(), 1);
        assert_eq!(s.skipped["隐藏文件/目录"], 2);
        assert_eq!(s.skipped["忽略的目录(node_modules)"], 1);
        assert_eq!(s.skipped["空文件"], 1);
        assert_eq!(s.skipped["不支持的类型(.zip)"], 2);
        assert_eq!(s.skipped["不支持的类型(无扩展名)"], 1);
        assert!(skip_lines(&s.skipped)
            .iter()
            .any(|l| l == "不支持的类型(.zip) ×2"));
    }

    #[cfg(unix)]
    #[test]
    fn 不跟随符号链接且不会死循环() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("r");
        touch(&root.join("a.md"), "x");
        // 目录链接指回上级:跟随的话会无限递归
        std::os::unix::fs::symlink(&root, root.join("loop")).unwrap();
        std::os::unix::fs::symlink(root.join("a.md"), root.join("link.md")).unwrap();
        let s = scan(&root);
        assert_eq!(s.files.len(), 1);
        assert_eq!(s.skipped["符号链接"], 2);
    }

    #[test]
    fn 深度超过上限的目录被跳过() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("r");
        // 第 8 层仍可进入,第 9 层不进入
        let mut deep = root.clone();
        for i in 0..MAX_DEPTH {
            deep = deep.join(format!("d{i}"));
        }
        touch(&deep.join("in.md"), "x");
        touch(&deep.join("d9/out.md"), "x");
        let s = scan(&root);
        assert_eq!(s.files.len(), 1);
        assert!(s.files[0].rel.ends_with("in.md"));
        assert_eq!(s.skipped[&format!("目录层级超过 {MAX_DEPTH} 层")], 1);
    }

    #[test]
    fn 文件数超过上限时截断并提示() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("r");
        for i in 0..(MAX_FILES + 5) {
            touch(&root.join(format!("f{i:05}.txt")), "x");
        }
        let s = scan(&root);
        assert_eq!(s.files.len(), MAX_FILES);
        assert_eq!(s.skipped[&format!("超过 {MAX_FILES} 个文件的上限")], 5);
    }
}
