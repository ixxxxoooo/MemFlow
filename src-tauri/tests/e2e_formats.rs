// @author ygw
//! 新增文档格式集成测试:xlsx 大表分块带表头、pptx/html/epub 插图关联、csv(GBK)、文件夹混合导入。

mod common;

use common::*;
use memflow_lib::core::Core;
use memflow_lib::ingest::{self, CaptureInput, CaptureResult};
use memflow_lib::{queue, repo};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn make_zip(path: &Path, entries: &[(&str, &[u8])]) {
    let file = fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default();
    for (name, data) in entries {
        zip.start_file(*name, opts).unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap();
}

async fn import(core: &Arc<Core>, paths: &[&Path]) -> CaptureResult {
    ingest::capture(
        core,
        CaptureInput {
            paths: paths.iter().map(|p| p.to_string_lossy().into()).collect(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
}

fn item(core: &Arc<Core>, id: i64) -> repo::ItemView {
    core.db.with(|c| repo::get_item(c, id)).unwrap().unwrap()
}

fn doc_of(core: &Arc<Core>, r: &CaptureResult) -> i64 {
    r.created
        .iter()
        .copied()
        .find(|id| item(core, *id).kind == "document")
        .unwrap()
}

/// 生成 300 行的最小 xlsx(行内字符串,无需共享字符串表)
fn make_big_xlsx(path: &Path) {
    let mut rows = String::from(
        r#"<row r="1"><c r="A1" t="inlineStr"><is><t>项目</t></is></c><c r="B1" t="inlineStr"><is><t>金额</t></is></c></row>"#,
    );
    for i in 2..=301 {
        rows.push_str(&format!(
            r#"<row r="{i}"><c r="A{i}" t="inlineStr"><is><t>事项第{n}行</t></is></c><c r="B{i}"><v>{v}</v></c></row>"#,
            n = i - 1,
            v = i * 10
        ));
    }
    let sheet = format!("<worksheet><sheetData>{rows}</sheetData></worksheet>");
    let wb = r#"<workbook xmlns:r="r"><sheets><sheet name="明细" sheetId="1" r:id="rId1"/></sheets></workbook>"#;
    let rels = r#"<Relationships><Relationship Id="rId1" Type="x/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#;
    make_zip(
        path,
        &[
            ("xl/workbook.xml", wb.as_bytes()),
            ("xl/_rels/workbook.xml.rels", rels.as_bytes()),
            ("xl/worksheets/sheet1.xml", sheet.as_bytes()),
        ],
    );
}

#[tokio::test]
async fn xlsx大表分块每块重复表头且可按行内容检索() {
    let (url, _m) = start_mock().await;
    let (core, dir) = new_core(&url);
    let p = dir.path().join("费用明细.xlsx");
    make_big_xlsx(&p);
    let r = import(&core, &[&p]).await;
    assert!(r.errors.is_empty(), "{r:?}");
    queue::drain_once(&core).await.unwrap();
    let id = r.created[0];
    assert_eq!(item(&core, id).status, "done");
    let chunks = core.db.with(|c| repo::list_chunks(c, id)).unwrap();
    assert!(chunks.len() > 1, "300 行应被切成多块: {}", chunks.len());
    for c in &chunks {
        assert!(
            c.text.contains("| 项目 | 金额 |"),
            "每块都带表头: {}",
            c.text
        );
        assert_eq!(c.heading.as_deref(), Some("明细"), "工作表名作为标题路径");
    }
    let ev = ask_in(&core, None, "事项第299行的金额是多少", None).await;
    let src = sources_of(&ev);
    let hit = src
        .iter()
        .find(|s| s.item_id == id)
        .expect("应命中表格切片");
    assert!(hit.text.contains("事项第299行"), "{}", hit.text);
}

#[tokio::test]
async fn pptx与html与epub的插图被关联() {
    let (url, _m) = start_mock().await;
    let (core, dir) = new_core(&url);
    let png = fs::read(fixture("ocr_budget.png")).unwrap();

    // pptx:一页带图
    let pptx = dir.path().join("汇报.pptx");
    let pres = r#"<p:presentation xmlns:r="r"><p:sldIdLst><p:sldId id="1" r:id="rId1"/></p:sldIdLst></p:presentation>"#;
    let prels = r#"<Relationships><Relationship Id="rId1" Type="x/slide" Target="slides/slide1.xml"/></Relationships>"#;
    let s1 = r#"<p:sld><p:cSld><p:spTree>
      <p:sp><p:nvSpPr><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:r><a:t>预算汇报</a:t></a:r></a:p></p:txBody></p:sp>
      <p:pic><p:nvPicPr><p:cNvPr id="2" name="图" descr="预算图"/></p:nvPicPr><p:blipFill><a:blip r:embed="rId5"/></p:blipFill></p:pic>
      </p:spTree></p:cSld></p:sld>"#;
    let srels = r#"<Relationships><Relationship Id="rId5" Type="x/image" Target="../media/image1.png"/></Relationships>"#;
    make_zip(
        &pptx,
        &[
            ("ppt/presentation.xml", pres.as_bytes()),
            ("ppt/_rels/presentation.xml.rels", prels.as_bytes()),
            ("ppt/slides/slide1.xml", s1.as_bytes()),
            ("ppt/slides/_rels/slide1.xml.rels", srels.as_bytes()),
            ("ppt/media/image1.png", png.as_slice()),
        ],
    );
    let r1 = import(&core, &[&pptx]).await;
    assert_eq!(r1.linked_images, 1, "{r1:?}");
    let d1 = doc_of(&core, &r1);
    let l1 = core.db.with(|c| repo::images_of(c, d1)).unwrap();
    assert_eq!(l1[0].anchor, "ppt/media/image1.png");

    // html:相对路径图片
    let html = dir.path().join("网页/页.html");
    fs::create_dir_all(html.parent().unwrap()).unwrap();
    fs::write(
        &html,
        "<html><head><title>页</title></head><body><p>说明</p><img src=\"pic.png\" alt=\"示意\"></body></html>",
    )
    .unwrap();
    fs::write(
        dir.path().join("网页/pic.png"),
        fs::read(fixture("ocr_route.png")).unwrap(),
    )
    .unwrap();
    let r2 = import(&core, &[&html]).await;
    assert_eq!(r2.linked_images, 1, "{r2:?}");
    let d2 = doc_of(&core, &r2);
    let l2 = core.db.with(|c| repo::images_of(c, d2)).unwrap();
    assert_eq!(l2[0].anchor, "pic.png");
    assert!(item(&core, d2).title.as_deref() == Some("页"));

    // epub:章节里的插图
    let epub = dir.path().join("书.epub");
    let container = r#"<container><rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>"#;
    let opf = r#"<package><manifest><item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#;
    let ch1 =
        r#"<html><body><h1>第一章</h1><p>看图</p><img src="img/a.png" alt="图一"/></body></html>"#;
    let png2: Vec<u8> = fs::read(fixture("ocr_budget.png")).unwrap();
    make_zip(
        &epub,
        &[
            ("META-INF/container.xml", container.as_bytes()),
            ("OEBPS/content.opf", opf.as_bytes()),
            ("OEBPS/ch1.xhtml", ch1.as_bytes()),
            ("OEBPS/img/a.png", png2.as_slice()),
        ],
    );
    let r3 = import(&core, &[&epub]).await;
    // 与 pptx 里的图片内容相同:库内去重,复用已有图片记录,不新建
    assert_eq!(r3.linked_images, 0, "{r3:?}");
    let d3 = doc_of(&core, &r3);
    let l3 = core.db.with(|c| repo::images_of(c, d3)).unwrap();
    assert_eq!(l3.len(), 1);
    assert_eq!(l3[0].child_id, l1[0].child_id, "同一张图片只存一份");
    assert_eq!(l3[0].anchor, "OEBPS/img/a.png");

    queue::drain_once(&core).await.unwrap();
    let c3 = core.db.with(|c| repo::list_chunks(c, d3)).unwrap();
    assert!(
        c3.iter().any(|c| !c.images.is_empty()),
        "epub 切片应挂上图片: {c3:?}"
    );
}

#[tokio::test]
async fn 文件夹混合格式全部可导入_csv按gbk() {
    let (url, _m) = start_mock().await;
    let (core, dir) = new_core(&url);
    let root = dir.path().join("资料");
    fs::create_dir_all(&root).unwrap();
    let (gbk, _, _) = encoding_rs::GBK.encode("姓名,部门\n王小明,研发中心\n");
    fs::write(root.join("人员.csv"), gbk).unwrap();
    fs::write(
        root.join("说明.rtf"),
        br"{\rtf1\ansi\ansicpg936 \'b2\'e2\'ca\'d4\par}",
    )
    .unwrap();
    fs::write(root.join("旧版.doc"), b"x").unwrap();
    fs::write(root.join("邮件.eml"), b"x").unwrap();
    let r = import(&core, &[&root]).await;
    assert_eq!(r.created.len(), 2, "{r:?}");
    // 旧版 doc / eml 不支持,按原因汇总到跳过
    assert!(
        r.skipped.iter().any(|l| l.contains(".doc")),
        "{:?}",
        r.skipped
    );
    assert!(
        r.skipped.iter().any(|l| l.contains(".eml")),
        "{:?}",
        r.skipped
    );
    queue::drain_once(&core).await.unwrap();
    let ev = ask_in(&core, None, "研发中心的王小明", None).await;
    assert!(sources_of(&ev).iter().any(|s| s.text.contains("王小明")));
}
