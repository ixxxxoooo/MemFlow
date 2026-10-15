// @author ixxxxoooo
//! 文件夹导入集成测试:递归、过滤、汇总、来源路径、去重、与知识库的配合。

mod common;

use common::*;
use memflow_lib::db::DEFAULT_KB_ID;
use memflow_lib::ingest::{self, CaptureInput};
use memflow_lib::kb;
use memflow_lib::repo;
use std::fs;
use std::path::Path;

fn write(p: &Path, content: &[u8]) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, content).unwrap();
}

/// 构造一个带子目录、隐藏文件、不支持类型与重复文件的文件夹
fn sample_folder(root: &Path) {
    write(&root.join("README.md"), "# 项目手册\n\n总览".as_bytes());
    write(
        &root.join("设计/架构.md"),
        "# 架构\n\n检索与入库".as_bytes(),
    );
    write(&root.join("设计/笔记.txt"), "一些零散的想法".as_bytes());
    write(&root.join("设计/副本.txt"), "一些零散的想法".as_bytes()); // 与笔记.txt 内容相同
    write(&root.join("素材/logo.png"), b"\x89PNG fake");
    write(&root.join("素材/包.zip"), b"zip");
    write(&root.join("素材/包2.zip"), b"zip");
    write(&root.join(".hidden/secret.md"), b"hidden");
    write(&root.join("node_modules/x/index.md"), b"dep");
}

#[tokio::test]
async fn 导入文件夹_递归收集并汇总导入重复跳过() {
    let (url, _m) = start_mock().await;
    let (core, dir) = new_core(&url);
    let root = dir.path().join("手册");
    sample_folder(&root);

    let r = ingest::capture(
        &core,
        CaptureInput {
            paths: vec![root.to_string_lossy().into()],
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // README、架构、笔记(副本被判为同批重复)、logo.png → 4 条新记录
    assert_eq!(r.created.len(), 4, "{r:?}");
    assert!(r.errors.is_empty(), "{r:?}");
    // 跳过:.hidden(1) + node_modules(1) + zip(2) + 同批重复(1) = 5
    assert_eq!(r.skipped_count, 5, "{:?}", r.skipped);
    assert!(
        r.skipped.iter().any(|l| l == "不支持的类型(.zip) ×2"),
        "{:?}",
        r.skipped
    );
    assert!(r.skipped.iter().any(|l| l.contains("node_modules")));
    assert!(r.skipped.iter().any(|l| l.contains("内容相同")));

    // 来源路径:文件夹名/相对路径
    let paths: Vec<String> = r
        .created
        .iter()
        .map(|id| {
            core.db
                .with(|c| repo::get_item(c, *id))
                .unwrap()
                .unwrap()
                .source_path
                .unwrap()
        })
        .collect();
    assert!(paths.contains(&"手册/README.md".to_string()), "{paths:?}");
    assert!(paths.contains(&"手册/设计/架构.md".to_string()));
    assert!(paths.contains(&"手册/素材/logo.png".to_string()));
}

#[tokio::test]
async fn 再次导入同一文件夹_全部判重复() {
    let (url, _m) = start_mock().await;
    let (core, dir) = new_core(&url);
    let root = dir.path().join("手册");
    sample_folder(&root);
    let input = || CaptureInput {
        paths: vec![root.to_string_lossy().into()],
        ..Default::default()
    };
    let first = ingest::capture(&core, input()).await.unwrap();
    let second = ingest::capture(&core, input()).await.unwrap();
    assert!(second.created.is_empty());
    // 副本.txt 与已入库的 笔记.txt 内容相同,第二次也被判为已存在
    assert_eq!(second.duplicates.len(), first.created.len() + 1);
}

#[tokio::test]
async fn 文件夹可导入到指定知识库且不影响其他库() {
    let (url, _m) = start_mock().await;
    let (core, dir) = new_core(&url);
    let kb_b = kb::create(&core, "资料", "").unwrap().id;
    let root = dir.path().join("手册");
    sample_folder(&root);
    let r = ingest::capture(
        &core,
        CaptureInput {
            paths: vec![root.to_string_lossy().into()],
            kb_id: Some(kb_b),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(!r.created.is_empty());
    for id in &r.created {
        assert_eq!(
            core.db
                .with(|c| repo::get_item(c, *id))
                .unwrap()
                .unwrap()
                .kb_id,
            kb_b
        );
    }
    let kbs = core.db.with(repo::list_kbs).unwrap();
    assert_eq!(
        kbs.iter()
            .find(|k| k.id == DEFAULT_KB_ID)
            .unwrap()
            .item_count,
        0
    );
}

#[tokio::test]
async fn 文件夹与单文件混合导入_空文件夹不报错() {
    let (url, _m) = start_mock().await;
    let (core, dir) = new_core(&url);
    let empty = dir.path().join("空");
    fs::create_dir_all(&empty).unwrap();
    let r = ingest::capture(
        &core,
        CaptureInput {
            paths: vec![empty.to_string_lossy().into()],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(r.created.is_empty() && r.errors.is_empty());

    let single = dir.path().join("单.md");
    fs::write(&single, "# 单文件").unwrap();
    let folder = dir.path().join("夹");
    write(&folder.join("a.md"), b"# a");
    let r = ingest::capture(
        &core,
        CaptureInput {
            paths: vec![
                single.to_string_lossy().into(),
                folder.to_string_lossy().into(),
            ],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(r.created.len(), 2);
}

#[tokio::test]
async fn 文件夹里的空文件算跳过_单独拖入空文件仍报错() {
    let (url, _m) = start_mock().await;
    let (core, dir) = new_core(&url);
    let folder = dir.path().join("夹");
    write(&folder.join("ok.md"), b"# ok");
    write(&folder.join("empty.md"), b"");
    let r = ingest::capture(
        &core,
        CaptureInput {
            paths: vec![folder.to_string_lossy().into()],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(r.created.len(), 1);
    assert!(r.errors.is_empty());
    assert_eq!(r.skipped_count, 1);

    let only = dir.path().join("空.md");
    fs::write(&only, "").unwrap();
    let err = ingest::capture(
        &core,
        CaptureInput {
            paths: vec![only.to_string_lossy().into()],
            ..Default::default()
        },
    )
    .await;
    assert!(err.is_err());
}
