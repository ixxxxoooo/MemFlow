// @author ixxxxoooo
//! 图文关联集成测试:md/docx 引用图片的关联与去重、图中文字(本机 OCR)并入文档切片、
//! 搜索图中文字命中文档、删除与转移时图片跟随文档。
//!
//! OCR 依赖 macOS Vision,相关断言只在 macOS 上执行;关联与去重逻辑全平台通用。

mod common;

use common::*;
use memflow_lib::core::Core;
use memflow_lib::db::DEFAULT_KB_ID;
use memflow_lib::ingest::{self, CaptureInput, CaptureResult};
use memflow_lib::{kb, queue, repo};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// 让 OCR 走与正式应用一致的"子进程"路径(测试程序自身不支持 --ocr-worker)
fn use_worker() {
    std::env::set_var("MEMFLOW_OCR_EXE", env!("CARGO_BIN_EXE_memflow"));
}

fn write(p: &Path, content: &[u8]) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, content).unwrap();
}

fn copy_fixture(name: &str, to: &Path) {
    write(to, &fs::read(fixture(name)).unwrap());
}

async fn import(core: &Arc<Core>, paths: &[&Path], kb: Option<i64>) -> CaptureResult {
    ingest::capture(
        core,
        CaptureInput {
            paths: paths.iter().map(|p| p.to_string_lossy().into()).collect(),
            kb_id: kb,
            ..Default::default()
        },
    )
    .await
    .unwrap()
}

fn item(core: &Arc<Core>, id: i64) -> repo::ItemView {
    core.db.with(|c| repo::get_item(c, id)).unwrap().unwrap()
}

fn count(core: &Arc<Core>, sql: &str) -> i64 {
    core.db
        .with(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
        .unwrap()
}

/// 构造最小 docx:正文先引用 image2(路线图)再引用 image1(预算图),验证按正文顺序而非文件顺序
fn make_docx(path: &Path) {
    let file = fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default();
    let doc = r#"<w:document xmlns:w="w" xmlns:a="a" xmlns:r="r"><w:body>
      <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>采购方案</w:t></w:r></w:p>
      <w:p><w:r><w:t>先看供应商路线图</w:t></w:r></w:p>
      <w:p><w:r><w:drawing><a:blip r:embed="rId11"/></w:drawing></w:r></w:p>
      <w:p><w:r><w:t>再看预算图</w:t></w:r></w:p>
      <w:p><w:r><w:drawing><a:blip r:embed="rId10"/></w:drawing></w:r></w:p>
    </w:body></w:document>"#;
    let rels = r#"<Relationships>
      <Relationship Id="rId10" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image1.png"/>
      <Relationship Id="rId11" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image2.png"/>
    </Relationships>"#;
    zip.start_file("word/document.xml", opts).unwrap();
    zip.write_all(doc.as_bytes()).unwrap();
    zip.start_file("word/_rels/document.xml.rels", opts)
        .unwrap();
    zip.write_all(rels.as_bytes()).unwrap();
    zip.start_file("word/media/image1.png", opts).unwrap();
    zip.write_all(&fs::read(fixture("ocr_budget.png")).unwrap())
        .unwrap();
    zip.start_file("word/media/image2.png", opts).unwrap();
    zip.write_all(&fs::read(fixture("ocr_route.png")).unwrap())
        .unwrap();
    zip.finish().unwrap();
}

#[tokio::test]
async fn md文件夹_相对路径图片被关联_重复引用与缺失图片() {
    use_worker();
    let (url, _m) = start_mock().await;
    let (core, dir) = new_core(&url);
    let root = dir.path().join("手册");
    copy_fixture("ocr_budget.png", &root.join("img/budget.png"));
    // 同一张图换个路径/文件名再放一份(内容相同 → 库内只存一条)
    copy_fixture("ocr_budget.png", &root.join("img/budget 副本.png"));
    write(
        &root.join("docs/指南.md"),
        "# 采购指南\n\n设备采购需要先做预算。\n\n![预算](../img/budget.png)\n\n再看一遍 ![重复](../img/budget%20副本.png)\n\n![缺失](../img/none.png) 和远程图 ![远程](https://e.com/x.png)\n"
            .as_bytes(),
    );

    let r = import(&core, &[&root], None).await;
    assert!(r.errors.is_empty(), "{r:?}");
    // 引用的图片找不到 = 1;远程图不计入跳过;img 目录里两张同内容图只导入一份(另一份同批重复)
    assert!(
        r.skipped.iter().any(|l| l.contains("引用的图片找不到")),
        "{:?}",
        r.skipped
    );
    queue::drain_once(&core).await.unwrap();

    let doc_id = *r
        .created
        .iter()
        .find(|id| item(&core, **id).kind == "document")
        .unwrap();
    let images = core.db.with(|c| repo::images_of(c, doc_id)).unwrap();
    // 两个不同写法 → 两条关联,指向同一张图片记录
    assert_eq!(images.len(), 2, "{images:?}");
    assert_eq!(images[0].child_id, images[1].child_id);
    assert_eq!(images[0].anchor, "../img/budget.png");
    assert_eq!(images[1].anchor, "../img/budget%20副本.png");
    // 图片记录只有一条(文件夹里的两份同内容图 + 两处引用 → 库内去重)
    assert_eq!(
        count(&core, "SELECT count(*) FROM items WHERE kind='image'"),
        1
    );
    let img = item(&core, images[0].child_id);
    // 文件夹里显式导入的图片不是"内嵌"图片:删除文档时不会被一并清理
    assert!(!img.embedded, "{img:?}");
    // 非 macOS 没有本机 OCR,又未启用视觉模型时图片没有文字线索,按设计只归档不索引
    let expected = if cfg!(target_os = "macos") {
        "done"
    } else {
        "archived_only"
    };
    assert_eq!(img.status, expected);

    if cfg!(target_os = "macos") {
        let ocr = img.ocr_text.clone().unwrap_or_default();
        assert!(
            ocr.contains("采购") && ocr.contains("预算"),
            "OCR 结果: {ocr:?}"
        );
        // 图中文字并入文档切片,并记录切片 ↔ 图片关系
        let detail = core.db.with(|c| repo::list_chunks(c, doc_id)).unwrap();
        let with_img: Vec<_> = detail.iter().filter(|c| !c.images.is_empty()).collect();
        assert!(!with_img.is_empty(), "{detail:?}");
        assert!(
            with_img[0].text.contains("图中文字:"),
            "{}",
            with_img[0].text
        );
        assert!(with_img[0].text.contains("采购"));
        assert!(!with_img[0].text.contains('\u{E001}'), "标记应被剔除");
        assert_eq!(with_img[0].images[0].id, img.id);

        // 搜图中文字 → 命中的是文档切片,并带出缩略图信息
        let ev = ask_in(&core, None, "总计八万八千元是哪个预算", None).await;
        let src = sources_of(&ev);
        let hit = src
            .iter()
            .find(|s| s.item_id == doc_id)
            .expect("应命中文档切片");
        assert!(!hit.images.is_empty(), "{hit:?}");
        // 图片本身也被索引,命中时标注来源文档
        let img_hit = src.iter().find(|s| s.item_id == img.id);
        if let Some(h) = img_hit {
            assert_eq!(h.via.as_deref(), Some("来自《指南》"));
        }
    }
}

#[tokio::test]
async fn 单文件md引用图片_内嵌图随文档删除而清理_被两篇文档共用时保留() {
    let (url, _m) = start_mock().await;
    let (core, dir) = new_core(&url);
    let root = dir.path();
    write(&root.join("a/pic.png"), b"fake-png-bytes-A");
    write(&root.join("a/甲.md"), "# 甲\n\n![图](pic.png)".as_bytes());
    write(
        &root.join("a/乙.md"),
        "# 乙\n\n同一张图 ![图](pic.png)".as_bytes(),
    );

    let r1 = import(&core, &[&root.join("a/甲.md")], None).await;
    assert_eq!(r1.created.len(), 2, "文档 + 自动导入的图片: {r1:?}");
    assert_eq!(r1.linked_images, 1);
    let r2 = import(&core, &[&root.join("a/乙.md")], None).await;
    // 第二篇文档复用库内已有图片,只新建文档本身
    assert_eq!(r2.created.len(), 1, "{r2:?}");
    assert_eq!(r2.linked_images, 0);

    let img_id = core
        .db
        .with(|c| {
            Ok(
                c.query_row("SELECT id FROM items WHERE kind='image'", [], |r| {
                    r.get::<_, i64>(0)
                })?,
            )
        })
        .unwrap();
    assert!(item(&core, img_id).embedded);
    let parents = core.db.with(|c| repo::parents_of(c, img_id)).unwrap();
    assert_eq!(parents.len(), 2);

    // 删掉其中一篇:另一篇仍引用,图片保留
    let doc1 = r1
        .created
        .iter()
        .copied()
        .find(|id| item(&core, *id).kind == "document")
        .unwrap();
    let doc2 = r2.created[0];
    ingest::delete_items(&core, &[doc1]).await.unwrap();
    assert_eq!(
        count(&core, "SELECT count(*) FROM items WHERE kind='image'"),
        1
    );
    // 再删最后一篇:内嵌图片成为孤儿,一并清理(含附件文件)
    let rel = item(&core, img_id).attachment_path.unwrap();
    assert!(core.paths.attachments.join(&rel).exists());
    ingest::delete_items(&core, &[doc2]).await.unwrap();
    assert_eq!(count(&core, "SELECT count(*) FROM items"), 0);
    assert!(!core.paths.attachments.join(&rel).exists());
}

#[tokio::test]
async fn 显式导入的图片不会被文档删除清理_且先引用后导入会转为普通图片() {
    let (url, _m) = start_mock().await;
    let (core, dir) = new_core(&url);
    let root = dir.path().join("库");
    write(&root.join("pic.png"), b"fake-png-bytes-B");
    write(&root.join("文.md"), "# 文\n\n![x](pic.png)".as_bytes());
    // 单独先导入文档(图片自动导入为内嵌),再显式导入这张图片 → 转为普通图片
    let r1 = import(&core, &[&root.join("文.md")], None).await;
    assert_eq!(r1.linked_images, 1);
    let r2 = import(&core, &[&root.join("pic.png")], None).await;
    assert_eq!(r2.duplicates.len(), 1, "{r2:?}");
    let img_id = r2.duplicates[0];
    assert!(!item(&core, img_id).embedded);
    let doc = r1
        .created
        .iter()
        .copied()
        .find(|id| item(&core, *id).kind == "document")
        .unwrap();
    ingest::delete_items(&core, &[doc]).await.unwrap();
    assert_eq!(
        count(&core, "SELECT count(*) FROM items WHERE kind='image'"),
        1
    );
}

#[tokio::test]
async fn docx内嵌图片按正文顺序关联并把图中文字并入切片() {
    use_worker();
    let (url, _m) = start_mock().await;
    let (core, dir) = new_core(&url);
    let docx = dir.path().join("采购方案.docx");
    make_docx(&docx);

    let r = import(&core, &[&docx], None).await;
    assert!(r.errors.is_empty(), "{r:?}");
    assert_eq!(r.linked_images, 2, "{r:?}");
    queue::drain_once(&core).await.unwrap();

    let doc = r
        .created
        .iter()
        .copied()
        .find(|id| item(&core, *id).kind == "document")
        .unwrap();
    let images = core.db.with(|c| repo::images_of(c, doc)).unwrap();
    assert_eq!(images.len(), 2);
    // 正文先引用 image2(路线图),再引用 image1(预算图)
    assert_eq!(images[0].anchor, "media/image2.png");
    assert_eq!(images[1].anchor, "media/image1.png");
    assert!(images[0]
        .original_name
        .as_deref()
        .unwrap()
        .contains("image2"));
    // 正文里保留了图片占位
    let content = item(&core, doc).content;
    assert!(content.contains("![](media/image2.png)"), "{content}");

    if cfg!(target_os = "macos") {
        let chunks = core.db.with(|c| repo::list_chunks(c, doc)).unwrap();
        let all: String = chunks
            .iter()
            .map(|c| c.text.clone())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(all.contains("星河科技"), "{all}");
        assert!(all.contains("采购"), "{all}");
        // 两张图都挂在切片上,路线图先于预算图出现
        let ids: Vec<i64> = chunks
            .iter()
            .flat_map(|c| c.images.iter().map(|i| i.id))
            .collect();
        assert!(ids.contains(&images[0].child_id) && ids.contains(&images[1].child_id));
        let pos = |kw: &str| all.find(kw).unwrap();
        assert!(pos("星河科技") < pos("设备采购"), "{all}");
    }
}

#[tokio::test]
async fn 转移文档时关联的图片跟随() {
    let (url, _m) = start_mock().await;
    let (core, dir) = new_core(&url);
    write(&dir.path().join("pic.png"), b"fake-png-bytes-C");
    write(
        &dir.path().join("文.md"),
        "# 文\n\n![x](pic.png)".as_bytes(),
    );
    let r = import(&core, &[&dir.path().join("文.md")], None).await;
    let doc = r
        .created
        .iter()
        .copied()
        .find(|id| item(&core, *id).kind == "document")
        .unwrap();
    let b = kb::create(&core, "B库", "").unwrap();
    let moved = kb::move_items(&core, &[doc], b.id).unwrap();
    assert_eq!(moved.moved.len(), 2, "文档与图片一起转移: {moved:?}");
    assert_eq!(
        count(
            &core,
            &format!("SELECT count(*) FROM items WHERE kb_id={}", b.id)
        ),
        2
    );
    assert_eq!(
        count(
            &core,
            &format!("SELECT count(*) FROM items WHERE kb_id={DEFAULT_KB_ID}")
        ),
        0
    );
}

#[tokio::test]
async fn 图片单独导入时本机ocr让文字可被搜索() {
    if !cfg!(target_os = "macos") {
        return;
    }
    use_worker();
    let (url, _m) = start_mock().await;
    let (core, dir) = new_core(&url);
    let p = dir.path().join("采购截图.png");
    copy_fixture("ocr_budget.png", &p);
    let r = import(&core, &[&p], None).await;
    queue::drain_once(&core).await.unwrap();
    let it = item(&core, r.created[0]);
    assert_eq!(it.status, "done", "{it:?}");
    assert!(
        it.ocr_text.clone().unwrap_or_default().contains("采购"),
        "{it:?}"
    );
    // 没有视觉模型描述也能索引:切片里带图中文字
    let chunks = core.db.with(|c| repo::list_chunks(c, it.id)).unwrap();
    assert!(chunks[0].text.contains("图中文字:"), "{}", chunks[0].text);
    let ev = ask_in(&core, None, "设备采购预算是多少", None).await;
    assert!(
        sources_of(&ev).iter().any(|s| s.item_id == it.id),
        "图片应被检索命中"
    );
}

#[tokio::test]
async fn 文件夹导入的图片与内嵌图不进时间流_库内视图含文档与文件夹图片() {
    use_worker();
    let (url, _m) = start_mock().await;
    let (core, dir) = new_core(&url);
    // 文件夹:一篇 md + 一张被引用的图
    let root = dir.path().join("手册");
    copy_fixture("ocr_budget.png", &root.join("img/budget.png"));
    write(
        &root.join("指南.md"),
        "# 指南\n\n![预算](img/budget.png)\n".as_bytes(),
    );
    // 单文件 md 引用的图:作为"内嵌图"自动导入
    let solo = dir.path().join("单篇");
    copy_fixture("ocr_route.png", &solo.join("route.png"));
    write(
        &solo.join("路线.md"),
        "# 路线\n\n![路线](route.png)\n".as_bytes(),
    );
    let r = import(&core, &[&root], None).await;
    let r2 = import(&core, &[&solo.join("路线.md")], None).await;
    assert!(r.errors.is_empty() && r2.errors.is_empty(), "{r:?} {r2:?}");
    queue::drain_once(&core).await.unwrap();
    // 再来一条随手记录和一张拖入的图片(无来源路径)
    let note = ingest::capture(
        &core,
        CaptureInput {
            text: Some("随手记一条".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let loose = dir.path().join("拖入.png");
    // 同内容图片在默认库已存在 → 换一张不同内容的
    copy_fixture("ocr_route.png", &loose);
    let _ = import(
        &core,
        &[&loose],
        Some(core.db.with(|c| repo::create_kb(c, "另一库", "")).unwrap()),
    )
    .await;
    queue::drain_once(&core).await.unwrap();

    let list = |view: &str| -> Vec<repo::ItemView> {
        core.db
            .with(|c| {
                repo::list_items(
                    c,
                    &repo::ItemFilter {
                        view: Some(view.into()),
                        limit: Some(200),
                        ..Default::default()
                    },
                )
            })
            .unwrap()
    };
    let timeline = list("timeline");
    // 时间流:只有随手记录与无来源路径的独立图片
    assert!(timeline.iter().any(|i| note.created.contains(&i.id)));
    assert!(timeline.iter().all(|i| !i.embedded), "{timeline:?}");
    assert!(
        timeline
            .iter()
            .all(|i| i.kind == "note" || i.source_path.is_none()),
        "{timeline:?}"
    );
    assert!(timeline.iter().all(|i| i.kind != "document"));
    // 库内视图:文档在、内嵌图不在
    let library = list("library");
    assert!(library.iter().any(|i| i.kind == "document"));
    assert!(library.iter().all(|i| !i.embedded && i.kind != "note"));
    // 知识库计数:默认库有 2 篇文档和若干切片
    let kbs = core.db.with(repo::list_kbs).unwrap();
    let def = kbs.iter().find(|k| k.id == DEFAULT_KB_ID).unwrap();
    assert_eq!(def.document_count, 2, "{def:?}");
    assert!(def.chunk_count > 0, "{def:?}");
    assert!(def.item_count >= def.document_count);
}
