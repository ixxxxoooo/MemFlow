// @author ixxxxoooo
//! 图文关联:找出文档里引用的图片、把图中文字并入对应切片、本机 OCR。
//!
//! 设计要点(参考 RAGFlow 的"图片随文本块走"思路):
//! - 导入时(原始路径还在)解析 md/html 的 `![](相对路径)` / `<img src>`、docx 的内嵌媒体,
//!   把图片作为独立的图片记录入库(库内按内容哈希去重),并写入 `item_links(文档→图片, anchor)`
//! - 索引文档时,在引用位置插入「[图片:名称] 描述 图中文字:...」一段并带上私有区标记,
//!   分块后据标记写入 `chunk_images`,再剔除标记 —— 搜图中文字就能命中文档切片,命中后可展示缩略图
//! - 图片本身也单独索引(命中时标注"来自《文档》")

use crate::chunker::ChunkPiece;
use crate::core::{permit, Core};
use crate::error::Result;
use crate::extract::{classify, FileKind};
use crate::pdf_native;
use crate::repo::{self, LinkedImage};
use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

/// 图片标记起止符(私有区字符):`MARK_OPEN 图片id MARK_CLOSE`
pub const MARK_OPEN: char = '\u{E001}';
/// 图片标记结束符
pub const MARK_CLOSE: char = '\u{E002}';
/// 单个文档最多自动导入的图片数
pub const MAX_IMAGES_PER_DOC: usize = 200;
/// 单张内嵌图片大小上限
pub const MAX_IMAGE_BYTES: usize = 30 * 1024 * 1024;
/// docx 内嵌图片小于该字节数视为装饰性小图标,不导入
pub const DOCX_MIN_IMAGE_BYTES: usize = 512;
/// 并入文档切片的单张图 OCR/描述文字上限(字符)
const INLINE_TEXT_MAX: usize = 1500;

/// 文档中的一处图片引用
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImgRef {
    /// 替代文字
    pub alt: String,
    /// 原文中的写法(去掉尖括号与首尾空白,未解码)
    pub src: String,
    /// 在原文中的字节范围
    pub start: usize,
    pub end: usize,
}

fn img_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // 1) Markdown: ![alt](src "title") 或 ![alt](<src with space>)
        // 2) HTML: <img ... src="..." ...>
        Regex::new(
            r#"!\[([^\]\n]*)\]\(\s*(<[^>\n]+>|[^)\s]+)(?:\s+(?:"[^"\n]*"|'[^'\n]*'))?\s*\)|<img\b[^>]*>"#,
        )
        .expect("正则非法")
    })
}

/// 读取 HTML/XML 标签里的属性值(双引号或单引号,大小写不敏感;不会把 `data-src` 当成 `src`)
pub fn html_attr_value(tag: &str, name: &str) -> Option<String> {
    html_attr(tag, name)
}

fn html_attr(tag: &str, name: &str) -> Option<String> {
    let re = Regex::new(&format!(
        r#"(?i)(?:^|[\s"'/<]){}\s*=\s*(?:"([^"]*)"|'([^']*)')"#,
        regex::escape(name)
    ))
    .ok()?;
    let c = re.captures(tag)?;
    c.get(1)
        .or_else(|| c.get(2))
        .map(|m| m.as_str().to_string())
}

/// 扫描文本里的全部图片引用(Markdown 与 HTML 写法),按出现顺序返回
pub fn scan_refs(text: &str) -> Vec<ImgRef> {
    let mut out = Vec::new();
    for cap in img_regex().captures_iter(text) {
        let whole = cap.get(0).expect("整体匹配");
        let (alt, src) = if let Some(s) = cap.get(2) {
            (
                cap.get(1).map(|m| m.as_str()).unwrap_or("").to_string(),
                s.as_str()
                    .trim_start_matches('<')
                    .trim_end_matches('>')
                    .trim()
                    .to_string(),
            )
        } else {
            let tag = whole.as_str();
            match html_attr(tag, "src") {
                // 地址里的 &amp; 等实体还原后再作为锚点,与 html 转文本时的写法一致
                Some(s) => (
                    html_attr(tag, "alt").unwrap_or_default(),
                    crate::formats::decode_entities(s.trim()),
                ),
                None => continue,
            }
        };
        if src.is_empty() {
            continue;
        }
        out.push(ImgRef {
            alt: alt.trim().to_string(),
            src,
            start: whole.start(),
            end: whole.end(),
        });
    }
    out
}

/// 是否远程或内嵌数据地址(不导入)
pub fn is_remote(src: &str) -> bool {
    let l = src.trim().to_lowercase();
    l.starts_with("http://")
        || l.starts_with("https://")
        || l.starts_with("data:")
        || l.starts_with("//")
        || l.starts_with("ftp:")
}

/// 百分号解码(如 `my%20pic.png`);非法序列原样保留
pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let h = std::str::from_utf8(&b[i + 1..i + 3])
                .ok()
                .and_then(|x| u8::from_str_radix(x, 16).ok());
            if let Some(v) = h {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 把引用解析为本地图片文件路径
///
/// 规则:去掉 `?query` / `#fragment`、百分号解码;相对路径相对于文档所在目录;
/// 必须是存在的、受支持类型的图片文件。
pub fn resolve_local(base_dir: &Path, src: &str) -> Option<PathBuf> {
    if is_remote(src) {
        return None;
    }
    let s = src.strip_prefix("file://").unwrap_or(src);
    let s = s.split(['?', '#']).next().unwrap_or(s);
    let decoded = percent_decode(s);
    let p = Path::new(&decoded);
    let full = if p.is_absolute() {
        p.to_path_buf()
    } else {
        base_dir.join(p)
    };
    if full.is_file() && classify(&full) == FileKind::Image {
        Some(full)
    } else {
        None
    }
}

/// 组装并入文档切片的图片文字:`[图片:名称] 描述 图中文字:…`
fn image_label(name: &str, desc: Option<&str>, ocr: Option<&str>) -> String {
    let one_line = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut s = format!("[图片:{name}]");
    if let Some(d) = desc.map(one_line).filter(|d| !d.is_empty()) {
        s.push_str(&format!(" 描述:{d}"));
    }
    if let Some(o) = ocr.map(one_line).filter(|o| !o.is_empty()) {
        s.push_str(&format!(" 图中文字:{o}"));
    }
    s.chars().take(INLINE_TEXT_MAX).collect()
}

/// 在文档原文的图片引用处插入图片文字与标记,得到用于分块的索引文本
///
/// 已关联的图片 → 单独成段 `MARK_OPEN id MARK_CLOSE [图片:名称] 描述 图中文字`;
/// 未关联的引用(远程图、找不到的图)→ 退化为 `[图片:替代文字]`,没有替代文字则移除。
pub fn inline_images(text: &str, links: &[LinkedImage]) -> String {
    let refs = scan_refs(text);
    if refs.is_empty() {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for r in refs {
        out.push_str(&text[last..r.start]);
        last = r.end;
        match links.iter().find(|l| l.anchor == r.src) {
            Some(l) => {
                let name = if !r.alt.is_empty() {
                    r.alt.clone()
                } else {
                    l.original_name.clone().unwrap_or_else(|| "图片".into())
                };
                out.push_str(&format!(
                    "\n\n{MARK_OPEN}{}{MARK_CLOSE}{}\n\n",
                    l.child_id,
                    image_label(&name, l.vision_desc.as_deref(), l.ocr_text.as_deref())
                ));
            }
            None if !r.alt.is_empty() => out.push_str(&format!("[图片:{}]", r.alt)),
            None => {}
        }
    }
    out.push_str(&text[last..]);
    out
}

/// 去掉文本里的图片标记(只剔除标记符本身与其中的 id)
pub fn strip_image_marks(text: &str) -> String {
    if !text.contains(MARK_OPEN) {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == MARK_OPEN {
            for d in chars.by_ref() {
                if d == MARK_CLOSE {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// 从分块结果里提取每个切片出现的图片 id,并把标记从切片文本中剔除
///
/// 返回: 与 pieces 等长,第 i 项为第 i 个切片里的图片 id(按出现顺序、去重)
pub fn take_image_marks(pieces: &mut [ChunkPiece]) -> Vec<Vec<i64>> {
    pieces
        .iter_mut()
        .map(|p| {
            let mut ids: Vec<i64> = Vec::new();
            let mut rest = p.text.as_str();
            while let Some(a) = rest.find(MARK_OPEN) {
                let after = &rest[a + MARK_OPEN.len_utf8()..];
                if let Some(b) = after.find(MARK_CLOSE) {
                    if let Ok(id) = after[..b].parse::<i64>() {
                        if !ids.contains(&id) {
                            ids.push(id);
                        }
                    }
                    rest = &after[b + MARK_CLOSE.len_utf8()..];
                } else {
                    break;
                }
            }
            if !ids.is_empty() {
                p.text = strip_image_marks(&p.text);
            }
            ids
        })
        .collect()
}

/// 确保图片记录有本机 OCR 文字(已识别则直接返回缓存)
///
/// OCR 只依赖系统 Vision,不调用任何模型;识别失败不影响导入,返回空串(下次重刷再试)。
///
/// 参数: core 核心;item_id 图片记录 id;rel_path 附件相对路径
/// 返回: 图中文字(无文字为空串)
pub async fn ensure_ocr(core: &Arc<Core>, item_id: i64, rel_path: &str) -> String {
    if let Ok(Some(t)) = core.db.with_read(|c| repo::get_ocr_text(c, item_id)) {
        return t;
    }
    if !pdf_native::AVAILABLE {
        return String::new();
    }
    let Ok(abs) = crate::ingest::attachment_abs(core, rel_path) else {
        return String::new();
    };
    let Ok(_cpu) = permit(&core.stages.cpu).await else {
        return String::new();
    };
    let r = tokio::task::spawn_blocking(move || pdf_native::image_ocr_text(&abs)).await;
    match r {
        Ok(Ok(text)) => {
            let text = crate::extract::clean_text(&text);
            if let Err(e) = core.db.with(|c| repo::set_ocr_text(c, item_id, &text)) {
                tracing::warn!("保存图片 OCR 文字失败: {e}");
            }
            text
        }
        Ok(Err(e)) => {
            tracing::warn!("图片 {item_id} OCR 失败: {e}");
            String::new()
        }
        Err(e) => {
            tracing::warn!("图片 {item_id} OCR 任务异常: {e}");
            String::new()
        }
    }
}

/// docx 内嵌的一张图片
#[derive(Debug, Clone)]
pub struct EmbeddedImage {
    /// 正文里的引用写法(关系表 Target,如 `media/image1.png`)
    pub anchor: String,
    /// 文件名(如 `image1.png`)
    pub name: String,
    pub bytes: Vec<u8>,
}

/// 把一张内嵌图片放入结果列表(过滤非位图格式、过小的装饰图与超大图)
///
/// 参数: anchor 正文里的引用写法(文件名取自其最后一段)
/// 返回: false 表示已达数量上限,调用方应停止继续添加
pub fn push_embedded(
    out: &mut Vec<EmbeddedImage>,
    skipped: &mut Vec<String>,
    anchor: &str,
    bytes: Vec<u8>,
) -> bool {
    let name = anchor.rsplit('/').next().unwrap_or(anchor).to_string();
    if classify(Path::new(&name)) != FileKind::Image {
        skipped.push("文档内嵌的矢量/不支持格式图片".to_string());
        return true;
    }
    if out.len() >= MAX_IMAGES_PER_DOC {
        skipped.push(format!("文档内嵌图片过多,仅导入前 {MAX_IMAGES_PER_DOC} 张"));
        return false;
    }
    if bytes.len() < DOCX_MIN_IMAGE_BYTES {
        return true;
    }
    if bytes.len() > MAX_IMAGE_BYTES {
        skipped.push("文档内嵌图片超过 30MB".to_string());
        return true;
    }
    out.push(EmbeddedImage {
        anchor: anchor.to_string(),
        name,
        bytes,
    });
    true
}

/// 读取 docx 关系表:rId → Target(只保留图片关系)
pub fn docx_rels(xml: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let tag = Regex::new(r"<Relationship\b[^>]*>").expect("正则非法");
    for m in tag.find_iter(xml) {
        let t = m.as_str();
        let id = html_attr(t, "Id");
        let target = html_attr(t, "Target");
        let ty = html_attr(t, "Type").unwrap_or_default();
        if let (Some(id), Some(target)) = (id, target) {
            if ty.ends_with("/image") {
                map.insert(id, target);
            }
        }
    }
    map
}

/// 按正文出现顺序读取 docx 内嵌图片(去重、过滤非位图格式与过小/过大的图、数量封顶)
///
/// 返回: (图片列表, 被跳过的说明)
pub fn docx_images(path: &Path) -> Result<(Vec<EmbeddedImage>, Vec<String>)> {
    use std::io::Read;
    let file = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| crate::error::AppError::Parse(format!("DOCX 不是有效的压缩包: {e}")))?;
    let read_entry = |zip: &mut zip::ZipArchive<std::fs::File>, name: &str, limit: u64| {
        let mut buf = Vec::new();
        zip.by_name(name)
            .ok()
            .and_then(|e| e.take(limit).read_to_end(&mut buf).ok().map(|_| buf))
    };
    let doc = read_entry(&mut zip, "word/document.xml", 80 * 1024 * 1024)
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default();
    let rels = read_entry(&mut zip, "word/_rels/document.xml.rels", 8 * 1024 * 1024)
        .map(|b| docx_rels(&String::from_utf8_lossy(&b)))
        .unwrap_or_default();
    let blip =
        Regex::new(r#"<a:blip\b[^>]*?\br:embed="([^"]+)"|<v:imagedata\b[^>]*?\br:id="([^"]+)""#)
            .expect("正则非法");
    let mut out: Vec<EmbeddedImage> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for cap in blip.captures_iter(&doc) {
        let rid = cap
            .get(1)
            .or_else(|| cap.get(2))
            .map(|m| m.as_str())
            .unwrap_or("");
        let Some(target) = rels.get(rid) else {
            continue;
        };
        if !seen.insert(target.clone()) {
            continue;
        }
        if out.len() >= MAX_IMAGES_PER_DOC {
            skipped.push(format!("文档内嵌图片过多,仅导入前 {MAX_IMAGES_PER_DOC} 张"));
            break;
        }
        // Target 通常是相对 word/ 的路径,也可能是以 / 开头的绝对路径
        let entry = match target.strip_prefix('/') {
            Some(abs) => abs.to_string(),
            None => format!("word/{target}"),
        };
        let Some(bytes) = read_entry(&mut zip, &entry, MAX_IMAGE_BYTES as u64 + 1) else {
            continue;
        };
        if !push_embedded(&mut out, &mut skipped, target, bytes) {
            break;
        }
    }
    Ok((out, skipped))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(id: i64, anchor: &str, name: &str, ocr: &str) -> LinkedImage {
        LinkedImage {
            child_id: id,
            anchor: anchor.into(),
            seq: 0,
            original_name: Some(name.into()),
            attachment_path: None,
            ocr_text: Some(ocr.into()),
            vision_desc: None,
        }
    }

    #[test]
    fn 扫描markdown与html图片() {
        let t = "前文 ![架构图](img/a.png) 中间 ![](<my pic.png> \"标题\") <img alt=\"流程\" src='x/b.jpg' width=3>\n![远程](https://e.com/c.png)";
        let r = scan_refs(t);
        assert_eq!(r.len(), 4);
        assert_eq!(
            (r[0].alt.as_str(), r[0].src.as_str()),
            ("架构图", "img/a.png")
        );
        assert_eq!(r[1].src, "my pic.png");
        assert_eq!((r[2].alt.as_str(), r[2].src.as_str()), ("流程", "x/b.jpg"));
        assert!(is_remote(&r[3].src));
        assert_eq!(&t[r[0].start..r[0].end], "![架构图](img/a.png)");
    }

    #[test]
    fn 百分号解码() {
        assert_eq!(percent_decode("my%20pic.png"), "my pic.png");
        assert_eq!(percent_decode("%E5%9B%BE.png"), "图.png");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("a%zzb"), "a%zzb");
        assert_eq!(percent_decode("%2"), "%2");
    }

    #[test]
    fn 解析本地路径() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("img")).unwrap();
        std::fs::write(dir.path().join("img/my pic.png"), b"x").unwrap();
        std::fs::write(dir.path().join("note.txt"), b"x").unwrap();
        let ok = resolve_local(dir.path(), "img/my%20pic.png?raw=1#top").unwrap();
        assert!(ok.ends_with("img/my pic.png"));
        assert!(resolve_local(dir.path(), "img/none.png").is_none());
        assert!(resolve_local(dir.path(), "note.txt").is_none());
        assert!(resolve_local(dir.path(), "https://e.com/a.png").is_none());
    }

    #[test]
    fn 并入图片文字并生成标记() {
        let text =
            "第一段\n\n![图1](a.png)\n\n第二段 ![](missing.png) 尾 ![远程图](https://x/y.png)";
        let links = vec![link(7, "a.png", "a.png", "预算 100\n差旅")];
        let s = inline_images(text, &links);
        assert!(
            s.contains(&format!(
                "{MARK_OPEN}7{MARK_CLOSE}[图片:图1] 图中文字:预算 100 差旅"
            )),
            "{s}"
        );
        assert!(s.contains("[图片:远程图]"));
        assert!(!s.contains("missing.png"), "{s}");
        // 无引用时原样返回
        assert_eq!(inline_images("纯文字", &[]), "纯文字");
    }

    #[test]
    fn 提取并剔除标记() {
        let mut pieces = vec![
            ChunkPiece {
                text: format!("前文\n\n{MARK_OPEN}3{MARK_CLOSE}[图片:a] 图中文字:x\n\n{MARK_OPEN}3{MARK_CLOSE}[图片:a]{MARK_OPEN}9{MARK_CLOSE}[图片:b]"),
                heading: String::new(),
                page: None,
            },
            ChunkPiece { text: "没有图".into(), heading: String::new(), page: None },
        ];
        let ids = take_image_marks(&mut pieces);
        assert_eq!(ids, vec![vec![3, 9], vec![]]);
        assert!(!pieces[0].text.contains(MARK_OPEN) && !pieces[0].text.contains(MARK_CLOSE));
        assert!(pieces[0].text.contains("[图片:a] 图中文字:x"));
        assert_eq!(
            strip_image_marks(&format!("a{MARK_OPEN}12{MARK_CLOSE}b")),
            "ab"
        );
    }

    #[test]
    fn 读取docx关系表() {
        let xml = r#"<Relationships>
          <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>
          <Relationship Target="media/image1.png" Id="rId5" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image"/>
        </Relationships>"#;
        let m = docx_rels(xml);
        assert_eq!(m.len(), 1);
        assert_eq!(m["rId5"], "media/image1.png");
    }
}
