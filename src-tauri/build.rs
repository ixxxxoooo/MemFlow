// @author ygw
// Tauri 构建脚本;macOS 上额外编译 PDFKit/Vision 的 Objective-C 桥接(扫描版 PDF 的 OCR)

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rerun-if-changed=native/pdf_ocr.m");
        cc::Build::new()
            .file("native/pdf_ocr.m")
            .flag("-fobjc-arc")
            .flag("-Wno-deprecated-declarations")
            .compile("memflow_pdf_ocr");
        for fw in ["Foundation", "AppKit", "CoreGraphics", "PDFKit", "Vision"] {
            println!("cargo:rustc-link-lib=framework={fw}");
        }
    }
    tauri_build::build()
}
