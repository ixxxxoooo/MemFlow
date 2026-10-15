// @author ygw
//! 原生 PDF 读取与 OCR:macOS 通过 PDFKit + Vision(见 `native/pdf_ocr.m`),其它平台为空实现。
//!
//! `PdfDoc` 不是线程安全的:一个实例只应在同一时刻被一个线程使用(外层用 Mutex 保证)。

use crate::error::{AppError, Result};
use crate::layout::OcrLine;
use std::path::Path;

/// 当前平台是否具备原生 PDF/OCR 能力
pub const AVAILABLE: bool = cfg!(target_os = "macos");

#[cfg(target_os = "macos")]
mod ffi {
    use std::os::raw::{c_char, c_long, c_uchar, c_void};
    extern "C" {
        pub fn mf_free(p: *mut c_void);
        pub fn mf_pdf_open(path: *const c_char) -> *mut c_void;
        pub fn mf_pdf_page_count(h: *mut c_void) -> c_long;
        pub fn mf_pdf_close(h: *mut c_void);
        pub fn mf_pdf_page_text(h: *mut c_void, idx: c_long) -> *mut c_char;
        pub fn mf_pdf_page_ocr_json(h: *mut c_void, idx: c_long) -> *mut c_char;
        pub fn mf_pdf_page_png(
            h: *mut c_void,
            idx: c_long,
            max_edge: f64,
            out_len: *mut usize,
        ) -> *mut c_uchar;
    }
}

/// 已打开的 PDF 文档
pub struct PdfDoc {
    #[cfg(target_os = "macos")]
    handle: *mut std::os::raw::c_void,
    /// 总页数
    pub page_count: usize,
}

// SAFETY: 句柄只在持有 &PdfDoc / &mut PdfDoc 的线程内使用,调用方用 Mutex 串行化访问
unsafe impl Send for PdfDoc {}

#[cfg(target_os = "macos")]
impl PdfDoc {
    /// 打开 PDF;无法解析或需要密码时返回 None
    pub fn open(path: &Path) -> Option<PdfDoc> {
        let c = std::ffi::CString::new(path.to_str()?).ok()?;
        // SAFETY: c 在调用期间有效
        let handle = unsafe { ffi::mf_pdf_open(c.as_ptr()) };
        if handle.is_null() {
            return None;
        }
        let n = unsafe { ffi::mf_pdf_page_count(handle) };
        if n < 0 {
            unsafe { ffi::mf_pdf_close(handle) };
            return None;
        }
        Some(PdfDoc {
            handle,
            page_count: n as usize,
        })
    }

    fn take_string(p: *mut std::os::raw::c_char) -> Option<String> {
        if p.is_null() {
            return None;
        }
        // SAFETY: p 由 ObjC 侧 strdup 返回,以 NUL 结尾;读取后立即释放
        let s = unsafe { std::ffi::CStr::from_ptr(p) }
            .to_string_lossy()
            .into_owned();
        unsafe { ffi::mf_free(p.cast()) };
        Some(s)
    }

    /// 第 idx 页的文本层
    pub fn page_text(&self, idx: usize) -> Option<String> {
        Self::take_string(unsafe { ffi::mf_pdf_page_text(self.handle, idx as _) })
    }

    /// 第 idx 页的 OCR 行
    ///
    /// Vision 在多线程同时识别时会偶发失败,且各线程争抢神经引擎反而更慢,
    /// 因此全进程串行执行 OCR,并在失败时重试一次。
    pub fn page_ocr_lines(&self, idx: usize) -> Result<Vec<OcrLine>> {
        static OCR_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _g = OCR_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut json = None;
        for _ in 0..2 {
            json = Self::take_string(unsafe { ffi::mf_pdf_page_ocr_json(self.handle, idx as _) });
            if json.is_some() {
                break;
            }
        }
        let json = json.ok_or_else(|| AppError::Parse(format!("第 {} 页 OCR 失败", idx + 1)))?;
        parse_ocr_json(&json)
    }

    /// 第 idx 页渲染为 PNG(长边不超过 max_edge 像素)
    pub fn page_png(&self, idx: usize, max_edge: f64) -> Option<Vec<u8>> {
        let mut len = 0usize;
        let p = unsafe { ffi::mf_pdf_page_png(self.handle, idx as _, max_edge, &mut len) };
        if p.is_null() {
            return None;
        }
        // SAFETY: p 指向 len 字节的 malloc 缓冲
        let v = unsafe { std::slice::from_raw_parts(p, len) }.to_vec();
        unsafe { ffi::mf_free(p.cast()) };
        Some(v)
    }
}

#[cfg(target_os = "macos")]
impl Drop for PdfDoc {
    fn drop(&mut self) {
        unsafe { ffi::mf_pdf_close(self.handle) };
    }
}

#[cfg(not(target_os = "macos"))]
impl PdfDoc {
    /// 非 macOS 平台没有原生 PDF 能力
    pub fn open(_path: &Path) -> Option<PdfDoc> {
        None
    }
    pub fn page_text(&self, _idx: usize) -> Option<String> {
        None
    }
    pub fn page_ocr_lines(&self, _idx: usize) -> Result<Vec<OcrLine>> {
        Err(AppError::Parse("当前平台不支持 OCR".into()))
    }
    pub fn page_png(&self, _idx: usize, _max_edge: f64) -> Option<Vec<u8>> {
        None
    }
}

/// 解析 ObjC 侧返回的 OCR JSON
pub fn parse_ocr_json(json: &str) -> Result<Vec<OcrLine>> {
    #[derive(serde::Deserialize)]
    struct Raw {
        lines: Vec<RawLine>,
    }
    #[derive(serde::Deserialize)]
    struct RawLine {
        t: String,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    }
    let raw: Raw = serde_json::from_str(json)
        .map_err(|e| AppError::Parse(format!("OCR 结果无法解析: {e}")))?;
    Ok(raw
        .lines
        .into_iter()
        .map(|l| OcrLine {
            text: l.t,
            x: l.x,
            y: l.y,
            w: l.w,
            h: l.h,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 解析ocr_json() {
        let lines =
            parse_ocr_json(r#"{"lines":[{"t":"你好","x":0.1,"y":0.2,"w":0.3,"h":0.04}]}"#).unwrap();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "你好");
        assert!((lines[0].w - 0.3).abs() < 1e-6);
        assert!(parse_ocr_json("不是json").is_err());
    }
}
