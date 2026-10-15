// @author ygw
// macOS 原生 PDF 读取与 OCR 桥接(PDFKit + Vision),通过 C ABI 暴露给 Rust。
//
// 约定:
// - 所有返回的 char* / 字节缓冲都由本文件 malloc,调用方必须用 mf_free 释放
// - 每个页面操作都包在 @autoreleasepool 中,避免长文档内存持续增长
// - 函数不抛异常;失败返回 NULL / -1

#import <AppKit/AppKit.h>
#import <CoreGraphics/CoreGraphics.h>
#import <Foundation/Foundation.h>
#import <PDFKit/PDFKit.h>
#import <Vision/Vision.h>
#include <stdlib.h>
#include <string.h>

static char *mf_dup_utf8(NSString *s) {
    if (s == nil) return NULL;
    const char *u = [s UTF8String];
    if (u == NULL) return NULL;
    return strdup(u);
}

void mf_free(void *p) { free(p); }

// 打开 PDF;加密且未解锁、或无法解析时返回 NULL。返回值为 +1 保留的 PDFDocument。
void *mf_pdf_open(const char *path) {
    @autoreleasepool {
        if (path == NULL) return NULL;
        NSURL *url = [NSURL fileURLWithPath:[NSString stringWithUTF8String:path]];
        PDFDocument *doc = [[PDFDocument alloc] initWithURL:url];
        if (doc == nil) return NULL;
        // 需要密码的文档视为不可读
        if (doc.isLocked) return NULL;
        return (void *)CFBridgingRetain(doc);
    }
}

long mf_pdf_page_count(void *handle) {
    if (handle == NULL) return -1;
    PDFDocument *doc = (__bridge PDFDocument *)handle;
    return (long)doc.pageCount;
}

void mf_pdf_close(void *handle) {
    if (handle != NULL) CFRelease(handle);
}

// 文本层:PDFKit 按阅读顺序输出,每个视觉行一行
char *mf_pdf_page_text(void *handle, long idx) {
    @autoreleasepool {
        if (handle == NULL) return NULL;
        PDFDocument *doc = (__bridge PDFDocument *)handle;
        PDFPage *page = [doc pageAtIndex:(NSUInteger)idx];
        if (page == nil) return NULL;
        NSString *s = page.string;
        return mf_dup_utf8(s == nil ? @"" : s);
    }
}

// 把页面渲染成白底 CGImage(调用方负责 CGImageRelease)。max_edge 为像素长边上限。
static CGImageRef mf_render_page(PDFPage *page, double max_edge) {
    NSRect box = [page boundsForBox:kPDFDisplayBoxCropBox];
    if (NSIsEmptyRect(box)) box = [page boundsForBox:kPDFDisplayBoxMediaBox];
    CGFloat w = box.size.width, h = box.size.height;
    if (w <= 1 || h <= 1) return NULL;
    NSInteger rot = ((page.rotation % 360) + 360) % 360;
    if (rot == 90 || rot == 270) { CGFloat t = w; w = h; h = t; }
    double scale = max_edge / MAX(w, h);
    if (scale < 1.0) scale = 1.0;   // 不缩小小页面,避免 OCR 精度下降
    if (scale > 4.0) scale = 4.0;
    size_t pw = (size_t)ceil(w * scale), ph = (size_t)ceil(h * scale);
    if (pw > 8192 || ph > 8192) return NULL;

    CGColorSpaceRef cs = CGColorSpaceCreateDeviceRGB();
    CGContextRef ctx = CGBitmapContextCreate(NULL, pw, ph, 8, 0, cs,
        kCGImageAlphaPremultipliedLast | kCGBitmapByteOrder32Big);
    CGColorSpaceRelease(cs);
    if (ctx == NULL) return NULL;
    CGContextSetRGBFillColor(ctx, 1, 1, 1, 1);
    CGContextFillRect(ctx, CGRectMake(0, 0, pw, ph));
    CGContextScaleCTM(ctx, scale, scale);
    // 让裁剪框左下角对齐原点
    CGContextTranslateCTM(ctx, -box.origin.x, -box.origin.y);
    [page drawWithBox:kPDFDisplayBoxCropBox toContext:ctx];
    CGImageRef img = CGBitmapContextCreateImage(ctx);
    CGContextRelease(ctx);
    return img;
}

// 对页面做 OCR,返回 JSON: {"lines":[{"t":"文字","x":..,"y":..,"w":..,"h":..},...]}
// 坐标为 Vision 的归一化坐标(原点左下角)。失败返回 NULL。
char *mf_pdf_page_ocr_json(void *handle, long idx) {
    @autoreleasepool {
        if (handle == NULL) return NULL;
        PDFDocument *doc = (__bridge PDFDocument *)handle;
        PDFPage *page = [doc pageAtIndex:(NSUInteger)idx];
        if (page == nil) return NULL;
        CGImageRef img = mf_render_page(page, 2400.0);
        if (img == NULL) return NULL;

        VNRecognizeTextRequest *req = [[VNRecognizeTextRequest alloc] init];
        req.recognitionLevel = VNRequestTextRecognitionLevelAccurate;
        req.usesLanguageCorrection = YES;
        if (@available(macOS 13.0, *)) {
            req.revision = VNRecognizeTextRequestRevision3;
            // 优先中文与英文;只保留当前系统支持的语言,避免设置不支持的语言导致失败
            NSArray<NSString *> *want = @[ @"zh-Hans", @"zh-Hant", @"en-US" ];
            NSError *langErr = nil;
            NSArray<NSString *> *sup = [req supportedRecognitionLanguagesAndReturnError:&langErr];
            if (sup != nil) {
                NSMutableArray *use = [NSMutableArray array];
                for (NSString *l in want) if ([sup containsObject:l]) [use addObject:l];
                if (use.count > 0) req.recognitionLanguages = use;
            }
        }
        VNImageRequestHandler *handler = [[VNImageRequestHandler alloc] initWithCGImage:img options:@{}];
        NSError *err = nil;
        BOOL ok = [handler performRequests:@[ req ] error:&err];
        CGImageRelease(img);
        if (!ok) {
            NSLog(@"[memflow] Vision OCR 失败: %@", err);
            return NULL;
        }

        NSMutableArray *lines = [NSMutableArray array];
        for (VNRecognizedTextObservation *o in req.results) {
            VNRecognizedText *top = [[o topCandidates:1] firstObject];
            if (top == nil || top.string.length == 0) continue;
            CGRect b = o.boundingBox;
            [lines addObject:@{
                @"t" : top.string,
                @"x" : @(b.origin.x),
                @"y" : @(b.origin.y),
                @"w" : @(b.size.width),
                @"h" : @(b.size.height),
            }];
        }
        NSData *data = [NSJSONSerialization dataWithJSONObject:@{ @"lines" : lines } options:0 error:nil];
        if (data == nil) return NULL;
        NSString *json = [[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding];
        return mf_dup_utf8(json);
    }
}

// 把页面渲染为 PNG 字节(供视觉大模型整页解析)。*out_len 返回长度。失败返回 NULL。
unsigned char *mf_pdf_page_png(void *handle, long idx, double max_edge, size_t *out_len) {
    @autoreleasepool {
        if (handle == NULL || out_len == NULL) return NULL;
        PDFDocument *doc = (__bridge PDFDocument *)handle;
        PDFPage *page = [doc pageAtIndex:(NSUInteger)idx];
        if (page == nil) return NULL;
        CGImageRef img = mf_render_page(page, max_edge);
        if (img == NULL) return NULL;
        NSBitmapImageRep *rep = [[NSBitmapImageRep alloc] initWithCGImage:img];
        CGImageRelease(img);
        NSData *png = [rep representationUsingType:NSBitmapImageFileTypePNG properties:@{}];
        if (png == nil) return NULL;
        unsigned char *buf = (unsigned char *)malloc(png.length);
        if (buf == NULL) return NULL;
        memcpy(buf, png.bytes, png.length);
        *out_len = png.length;
        return buf;
    }
}
