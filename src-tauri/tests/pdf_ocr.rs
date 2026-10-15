// @author ygw
//! PDF 原生解析与 OCR 集成测试(仅 macOS:依赖 PDFKit + Vision)。

#![cfg(target_os = "macos")]

use memflow_lib::layout::{self, is_scanned};
use memflow_lib::pdf_native::PdfDoc;
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// 让 OCR 走与正式应用一致的"子进程"路径(测试程序自身不支持 --ocr-worker)
fn use_worker() {
    std::env::set_var("MEMFLOW_OCR_EXE", env!("CARGO_BIN_EXE_memflow"));
}

/// OCR 并还原整页 Markdown
fn ocr_markdown(name: &str) -> String {
    use_worker();
    let doc = PdfDoc::open(&fixture(name)).expect("打开 PDF 失败");
    assert_eq!(doc.page_count, 1);
    let lines = doc.page_ocr_lines(0).expect("OCR 失败");
    layout::lines_to_markdown(&lines)
}

#[test]
fn 文本层pdf无需ocr() {
    let doc = PdfDoc::open(&fixture("text_en.pdf")).unwrap();
    let text = doc.page_text(0).unwrap();
    assert!(text.contains("Quarterly Review"), "{text}");
    // 英文夹具文本很短,仍应识别出有文本层
    assert!(text.chars().filter(|c| !c.is_whitespace()).count() > 20);
}

#[test]
fn 扫描页没有文本层且ocr出中文() {
    let doc = PdfDoc::open(&fixture("scan_cn.pdf")).unwrap();
    let text = doc.page_text(0).unwrap_or_default();
    assert!(is_scanned(&text), "扫描页不应有文本层: {text:?}");
    let md = ocr_markdown("scan_cn.pdf");
    println!("{md}");
    for kw in ["预算", "差旅", "星河科技"] {
        assert!(md.contains(kw), "OCR 结果缺少「{kw}」:\n{md}");
    }
}

#[test]
fn 扫描页表格还原为markdown表格() {
    let md = ocr_markdown("scan_table.pdf");
    println!("{md}");
    assert!(md.contains('|'), "应识别出表格:\n{md}");
    for kw in ["差旅", "1200", "李四"] {
        assert!(md.contains(kw), "表格缺少「{kw}」:\n{md}");
    }
}

#[test]
fn 旋转页被转正后识别() {
    let md = ocr_markdown("scan_rotated.pdf");
    println!("{md}");
    assert!(
        md.contains("预算") && md.contains("差旅"),
        "旋转页识别失败:\n{md}"
    );
}

#[test]
fn 页面可渲染为png() {
    let doc = PdfDoc::open(&fixture("scan_cn.pdf")).unwrap();
    let png = doc.page_png(0, 800.0).expect("渲染失败");
    assert_eq!(&png[..4], &[0x89, b'P', b'N', b'G']);
    assert!(png.len() > 1000);
}

#[test]
fn 损坏文件返回none() {
    let tmp = std::env::temp_dir().join("memflow_bad.pdf");
    std::fs::write(&tmp, b"not a pdf").unwrap();
    assert!(PdfDoc::open(&tmp).is_none());
}
