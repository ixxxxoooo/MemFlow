// @author ixxxxoooo
//! AI 整理集成测试:单条整理(预览/确认/撤销/过期保护)、向量聚类、归并(来源关系/引用校验/可检索)。

mod common;

use common::*;
use memflow_lib::core::Core;
use memflow_lib::db::DEFAULT_KB_ID;
use memflow_lib::kb;
use memflow_lib::organize::{self, GroupOptions};
use memflow_lib::queue;
use memflow_lib::repo;
use std::sync::Arc;

const N1: &str = "老王说A项目的预算下个月会缩减20%,需要重新排期";
const N2: &str = "A项目预算缩减20%之后,老王建议重新排期";
const N3: &str = "老王提到A项目预算缩减20%,排期要重新安排";
const OTHER: &str = "今天晚饭吃了红烧肉和米饭,味道不错";

fn item(core: &Arc<Core>, id: i64) -> repo::ItemView {
    core.db.with(|c| repo::get_item(c, id)).unwrap().unwrap()
}

fn loose() -> GroupOptions {
    GroupOptions {
        threshold: Some(0.35),
        ..Default::default()
    }
}

#[tokio::test]
async fn 单条整理_预览不落库_确认覆盖并可撤销() {
    let (url, _m) = start_mock().await;
    let (core, _d) = new_core(&url);
    let id = put_in(&core, N1, DEFAULT_KB_ID).await;
    queue::drain_once(&core).await.unwrap();

    let p = organize::propose_single(&core, id, None).await.unwrap();
    assert_eq!(p.content, format!("整理:{N1}"));
    assert_eq!(p.title, "整理后标题");
    assert_eq!(p.notes, vec!["日期不明确"]);
    assert!(
        p.warnings.is_empty(),
        "数字没变不应有警告: {:?}",
        p.warnings
    );
    assert!(!p.cloud);
    // 预览阶段不写入
    assert_eq!(item(&core, id).content, N1);
    assert!(core
        .db
        .with(|c| repo::list_revisions(c, id))
        .unwrap()
        .is_empty());

    organize::apply_single(&core, id, &p.title, &p.content, p.base_updated_at).unwrap();
    let it = item(&core, id);
    assert_eq!(it.content, p.content);
    assert_eq!(it.title.as_deref(), Some("整理后标题"));
    assert_eq!(
        core.db.with(|c| repo::list_revisions(c, id)).unwrap().len(),
        1
    );
    // 覆盖后重新索引,新文本可被检索
    queue::drain_once(&core).await.unwrap();
    assert_eq!(item(&core, id).status, "done");
    let src = sources_of(&ask_in(&core, None, "整理:老王说A项目的预算", None).await);
    assert!(src
        .iter()
        .any(|s| s.item_id == id && s.text.contains("整理:")));

    // 撤销:标题恢复为空,原文恢复
    organize::undo_single(&core, id).unwrap();
    let it = item(&core, id);
    assert_eq!(it.content, N1);
    assert_eq!(it.title, None);
    assert!(core
        .db
        .with(|c| repo::list_revisions(c, id))
        .unwrap()
        .is_empty());
    assert!(
        organize::undo_single(&core, id).is_err(),
        "没有修订时撤销应报错"
    );
}

#[tokio::test]
async fn 单条整理_原文期间被改动则拒绝覆盖_数字被篡改会警告() {
    let (url, _m) = start_mock().await;
    let (core, _d) = new_core(&url);
    let id = put_in(&core, "预算缩减20%,改数字测试", DEFAULT_KB_ID).await;
    queue::drain_once(&core).await.unwrap();
    let p = organize::propose_single(&core, id, None).await.unwrap();
    assert!(
        p.warnings.iter().any(|w| w.contains("25")),
        "新增数字应被发现: {:?}",
        p.warnings
    );
    assert!(p
        .warnings
        .iter()
        .any(|w| w.contains("丢失") && w.contains("20")));

    // 方案生成后用户又改了原文
    memflow_lib::ingest::update_content(&core, id, "预算缩减20%,已经改过了").unwrap();
    let err = organize::apply_single(&core, id, &p.title, &p.content, p.base_updated_at)
        .unwrap_err()
        .to_string();
    assert!(err.contains("已被修改"), "{err}");
    assert!(core
        .db
        .with(|c| repo::list_revisions(c, id))
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn 单条整理_只支持笔记() {
    let (url, _m) = start_mock().await;
    let (core, dir) = new_core(&url);
    let f = dir.path().join("a.md");
    std::fs::write(&f, "# 标题\n正文内容").unwrap();
    let r = memflow_lib::ingest::capture(
        &core,
        memflow_lib::ingest::CaptureInput {
            paths: vec![f.to_string_lossy().into()],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    queue::drain_once(&core).await.unwrap();
    let err = organize::propose_single(&core, r.created[0], None)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("文字笔记"), "{err}");
}

async fn four_notes() -> (Arc<Core>, tempfile::TempDir, [i64; 3], i64) {
    let (url, _m) = start_mock().await;
    let (core, dir) = new_core(&url);
    let a = put_in(&core, N1, DEFAULT_KB_ID).await;
    let b = put_in(&core, N2, DEFAULT_KB_ID).await;
    let c = put_in(&core, N3, DEFAULT_KB_ID).await;
    let o = put_in(&core, OTHER, DEFAULT_KB_ID).await;
    queue::drain_once(&core).await.unwrap();
    (core, dir, [a, b, c], o)
}

#[tokio::test]
async fn 聚类_相近碎片成组_无关内容不入组() {
    let (core, _d, [a, b, c], o) = four_notes().await;
    let groups = organize::suggest_groups(&core, &loose()).unwrap();
    assert_eq!(groups.len(), 1, "{groups:?}");
    let mut ids = groups[0].ids.clone();
    ids.sort();
    assert_eq!(ids, vec![a, b, c]);
    assert!(!groups[0].ids.contains(&o));
    assert!(groups[0].avg_similarity > 0.35);
    // 提高组大小下限后不再有建议
    let none = organize::suggest_groups(
        &core,
        &GroupOptions {
            min_size: Some(4),
            ..loose()
        },
    )
    .unwrap();
    assert!(none.is_empty());
    // 阈值很高时也不成组
    let strict = organize::suggest_groups(
        &core,
        &GroupOptions {
            threshold: Some(0.999),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(strict.is_empty());
}

#[tokio::test]
async fn 聚类_不同知识库的碎片不会被归到一组() {
    let (url, _m) = start_mock().await;
    let (core, _d) = new_core(&url);
    let kb2 = kb::create(&core, "读书", "").unwrap().id;
    put_in(&core, N1, DEFAULT_KB_ID).await;
    put_in(&core, N2, DEFAULT_KB_ID).await;
    put_in(&core, N3, kb2).await;
    queue::drain_once(&core).await.unwrap();
    assert!(organize::suggest_groups(&core, &loose())
        .unwrap()
        .is_empty());
    let g = organize::suggest_groups(
        &core,
        &GroupOptions {
            min_size: Some(2),
            ..loose()
        },
    )
    .unwrap();
    assert_eq!(g.len(), 1);
    assert_eq!(g[0].ids.len(), 2, "只有默认库的两条成组");
}

#[tokio::test]
async fn 可整理提示_只对高度相似的碎片出现() {
    let (core, _d, [a, b, _c], o) = four_notes().await;
    // 提示阈值较严:N1 与 N2 的哈希向量相似度可能低于它,这里只验证无关内容没有提示、且提示互为同伴
    let hints = organize::peer_hints(&core, &[a, b, o]).unwrap();
    assert!(!hints.contains_key(&o), "无关内容不应有提示: {hints:?}");
    for (id, peers) in &hints {
        assert!(!peers.contains(id), "不应把自己当同伴");
    }
}

#[tokio::test]
async fn 归并_生成主题笔记并记录来源_原记录保留_引用越界被移除() {
    let (core, _d, [a, b, c], _o) = four_notes().await;
    let mut ids = vec![c, a, b];
    let p = organize::propose_merge(&core, &ids, None).await.unwrap();
    // 来源按创建时间排序编号
    assert_eq!(p.ids, vec![a, b, c]);
    assert_eq!(p.sources.len(), 3);
    assert_eq!(p.title, "A项目预算");
    assert_eq!(p.content, "A项目预算缩减20%[1][2]", "越界的 [9] 被移除");
    assert!(
        p.warnings.iter().any(|w| w.contains("[9]")),
        "{:?}",
        p.warnings
    );
    assert!(!p.cloud);
    // 预览阶段没有新增记录
    assert_eq!(
        core.db
            .with(|c| repo::list_items(c, &Default::default()))
            .unwrap()
            .len(),
        4
    );

    ids = p.ids.clone();
    let new_id = organize::apply_merge(&core, &ids, &p.title, &p.content).unwrap();
    let topic = item(&core, new_id);
    assert_eq!(topic.kind, "note");
    assert_eq!(topic.title.as_deref(), Some("A项目预算"));
    assert!(topic.content.starts_with("A项目预算缩减20%[1][2]"));
    assert!(topic.content.contains("来源记录:") && topic.content.contains(&format!("记录 #{a}")));
    assert_eq!(topic.kb_id, DEFAULT_KB_ID);

    // 关系:主题笔记 → 三条来源;来源 → 主题笔记
    let srcs = core.db.with(|c| repo::merged_sources(c, new_id)).unwrap();
    assert_eq!(srcs.iter().map(|s| s.id).collect::<Vec<_>>(), vec![a, b, c]);
    let into = core.db.with(|c| repo::merged_into(c, b)).unwrap();
    assert_eq!(into.len(), 1);
    assert_eq!(into[0].id, new_id);
    // 原记录保持不变
    assert_eq!(item(&core, a).content, N1);

    // 新笔记走正常入库,可被检索
    queue::drain_once(&core).await.unwrap();
    assert_eq!(item(&core, new_id).status, "done");
    let hits = sources_of(&ask_in(&core, None, "A项目预算缩减", None).await);
    assert!(hits.iter().any(|s| s.item_id == new_id));

    // 已归并的来源与归并产物不再出现在建议里
    assert!(organize::suggest_groups(&core, &loose())
        .unwrap()
        .is_empty());

    // 删除主题笔记后,来源恢复为"未归并",又可再次被建议
    memflow_lib::ingest::delete_items(&core, &[new_id])
        .await
        .unwrap();
    assert!(core
        .db
        .with(|c| repo::merged_into(c, a))
        .unwrap()
        .is_empty());
    assert_eq!(organize::suggest_groups(&core, &loose()).unwrap().len(), 1);
}

#[tokio::test]
async fn 归并_参数校验() {
    let (url, _m) = start_mock().await;
    let (core, dir) = new_core(&url);
    let kb2 = kb::create(&core, "读书", "").unwrap().id;
    let a = put_in(&core, N1, DEFAULT_KB_ID).await;
    let b = put_in(&core, N2, kb2).await;
    queue::drain_once(&core).await.unwrap();

    let e1 = organize::propose_merge(&core, &[a], None)
        .await
        .unwrap_err()
        .to_string();
    assert!(e1.contains("至少选择 2 条"), "{e1}");
    let e2 = organize::propose_merge(&core, &[a, b], None)
        .await
        .unwrap_err()
        .to_string();
    assert!(e2.contains("同一个知识库"), "{e2}");
    let e3 = organize::apply_merge(&core, &[a, 9999], "t", "c")
        .unwrap_err()
        .to_string();
    assert!(e3.contains("已被删除"), "{e3}");
    let e4 = organize::apply_merge(&core, &[a, b], "t", " ")
        .unwrap_err()
        .to_string();
    assert!(e4.contains("不能为空"), "{e4}");
    drop(dir);
}

#[tokio::test]
async fn 可整理提示_几乎相同的两条互为同伴() {
    let (url, _m) = start_mock().await;
    let (core, _d) = new_core(&url);
    let a = put_in(&core, N1, DEFAULT_KB_ID).await;
    let b = put_in(&core, &format!("{N1}(补充一句)"), DEFAULT_KB_ID).await;
    let o = put_in(&core, OTHER, DEFAULT_KB_ID).await;
    queue::drain_once(&core).await.unwrap();
    let hints = organize::peer_hints(&core, &[a, b, o]).unwrap();
    assert_eq!(hints.get(&a), Some(&vec![b]), "{hints:?}");
    assert_eq!(hints.get(&b), Some(&vec![a]));
    assert!(!hints.contains_key(&o));
}
