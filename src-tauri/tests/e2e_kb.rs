// @author ixxxxoooo
//! 多知识库集成测试:检索隔离、范围记忆、拒答文案、转移后检索同步、库内去重、删库。

mod common;

use common::*;
use memflow_lib::db::DEFAULT_KB_ID;
use memflow_lib::ingest::{self, CaptureInput};
use memflow_lib::kb::{self, DeleteMode};
use memflow_lib::queue;
use memflow_lib::repo;
use memflow_lib::retrieval::{hybrid_search, SearchOptions};

const A_TEXT: &str = "老王说A项目的预算下个月会缩减20%,需要重新排期";
const B_TEXT: &str = "读书笔记:老王推荐的预算管理方法,强调按季度滚动复盘";

/// 两个知识库各一条相关内容
async fn two_kbs() -> (
    std::sync::Arc<memflow_lib::core::Core>,
    tempfile::TempDir,
    i64,
    i64,
    i64,
    i64,
) {
    let (url, _mock) = start_mock().await;
    let (core, dir) = new_core(&url);
    let kb_b = kb::create(&core, "读书", "").unwrap().id;
    let a = put_in(&core, A_TEXT, DEFAULT_KB_ID).await;
    let b = put_in(&core, B_TEXT, kb_b).await;
    queue::drain_once(&core).await.unwrap();
    (core, dir, DEFAULT_KB_ID, kb_b, a, b)
}

#[tokio::test]
async fn 限定知识库只返回库内来源_不限定则两库都搜() {
    let (core, _d, kb_a, kb_b, a, b) = two_kbs().await;

    let in_a = sources_of(&ask_in(&core, None, "老王说预算怎么样", Some(&[kb_a])).await);
    assert!(!in_a.is_empty());
    assert!(
        in_a.iter().all(|s| s.item_id == a),
        "A 范围不应出现 B 的内容: {in_a:?}"
    );
    assert!(in_a
        .iter()
        .all(|s| s.kb_name.as_deref() == Some("默认知识库")));

    let in_b = sources_of(&ask_in(&core, None, "老王说预算怎么样", Some(&[kb_b])).await);
    assert!(!in_b.is_empty());
    assert!(in_b.iter().all(|s| s.item_id == b));
    assert!(in_b.iter().all(|s| s.kb_name.as_deref() == Some("读书")));

    // 不选(None)与空数组都表示全部知识库
    for scope in [None, Some(&[][..])] {
        let all = sources_of(&ask_in(&core, None, "老王说预算怎么样", scope).await);
        let ids: Vec<i64> = all.iter().map(|s| s.item_id).collect();
        assert!(
            ids.contains(&a) && ids.contains(&b),
            "全部范围应同时命中两库: {ids:?}"
        );
    }
    // 显式多选两个库同样如此
    let both = sources_of(&ask_in(&core, None, "老王说预算怎么样", Some(&[kb_a, kb_b])).await);
    assert!(both.len() >= 2);
}

#[tokio::test]
async fn 范围保存到会话且后续沿用() {
    let (core, _d, _kb_a, kb_b, _a, b) = two_kbs().await;
    let ev = ask_in(&core, None, "老王说预算怎么样", Some(&[kb_b])).await;
    let sid = core.db.with(|c| Ok(repo::list_sessions(c)?[0].id)).unwrap();
    assert_eq!(
        core.db.with(|c| repo::get_session_kbs(c, sid)).unwrap(),
        vec![kb_b]
    );
    assert!(sources_of(&ev).iter().all(|s| s.item_id == b));
    // 追问不再传范围(None):沿用会话里保存的范围
    let ev2 = ask_in(&core, Some(sid), "老王说预算怎么样", None).await;
    assert!(sources_of(&ev2).iter().all(|s| s.item_id == b));
    // 显式传空数组 = 改回全部,并写回会话
    ask_in(&core, Some(sid), "老王说预算怎么样", Some(&[])).await;
    assert!(core
        .db
        .with(|c| repo::get_session_kbs(c, sid))
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn 范围内无资料拒答且点明范围() {
    let (core, _d, _kb_a, kb_b, _a, _b) = two_kbs().await;
    // 提问内容与读书库毫无关系
    let ev = ask_in(&core, None, "量子计算机的原理", Some(&[kb_b])).await;
    assert!(sources_of(&ev).is_empty());
    let ans = answer_of(&ev);
    assert!(ans.contains("所选知识库") && ans.contains("读书"), "{ans}");
    assert!(ans.contains("@"), "应提示可以去掉 @ 限定: {ans}");
}

#[tokio::test]
async fn 选择已删除的知识库会提示重选() {
    let (core, _d, ..) = two_kbs().await;
    let r = memflow_lib::chat::ask_in(&core, None, "老王说预算怎么样", None, Some(&[999]), &|_| {})
        .await;
    assert!(r.is_err());
}

#[tokio::test]
async fn 转移后向量与全文检索同步() {
    let (core, _d, kb_a, kb_b, a, _b) = two_kbs().await;
    let opts = |kbs: Vec<i64>| SearchOptions {
        top_n: 5,
        threshold: 0.12,
        kb_ids: Some(kbs),
    };
    let hits =
        |h: Vec<memflow_lib::retrieval::Hit>| h.into_iter().map(|x| x.item_id).collect::<Vec<_>>();

    assert!(hits(
        hybrid_search(&core, "A项目的预算", &opts(vec![kb_a]))
            .await
            .unwrap()
    )
    .contains(&a));
    assert!(!hits(
        hybrid_search(&core, "A项目的预算", &opts(vec![kb_b]))
            .await
            .unwrap()
    )
    .contains(&a));

    let embeds_before = core.vindex.len();
    let r = kb::move_items(&core, &[a], kb_b).unwrap();
    assert_eq!(r.moved, vec![a]);
    assert_eq!(core.vindex.len(), embeds_before, "转移不应丢失或重做向量");

    assert!(hits(
        hybrid_search(&core, "A项目的预算", &opts(vec![kb_b]))
            .await
            .unwrap()
    )
    .contains(&a));
    assert!(!hits(
        hybrid_search(&core, "A项目的预算", &opts(vec![kb_a]))
            .await
            .unwrap()
    )
    .contains(&a));

    // 仅全文路径:查询向量化失败(Ollama 不可达)时,范围同样生效
    let mut s = core.settings();
    s.ollama_url = "http://127.0.0.1:9".into();
    core.update_settings(s).unwrap();
    let fts_b = hits(
        hybrid_search(&core, "A项目 预算 缩减", &opts(vec![kb_b]))
            .await
            .unwrap(),
    );
    let fts_a = hits(
        hybrid_search(&core, "A项目 预算 缩减", &opts(vec![kb_a]))
            .await
            .unwrap(),
    );
    assert!(fts_b.contains(&a), "全文路径应在目标库找到: {fts_b:?}");
    assert!(!fts_a.contains(&a), "全文路径不应在原库找到: {fts_a:?}");
}

#[tokio::test]
async fn 重启后向量索引保留知识库归属() {
    let (core, _d, kb_a, kb_b, a, _b) = two_kbs().await;
    kb::move_items(&core, &[a], kb_b).unwrap();
    // 模拟重启:从数据库重新载入向量
    let model = core.settings().embed_model;
    core.vindex.load_from_db(&core.db, &model).unwrap();
    let v = common::fake_embed(A_TEXT);
    let in_b: Vec<i64> = core
        .vindex
        .search_in(&v, 5, Some(&[kb_b]))
        .iter()
        .map(|h| h.item_id)
        .collect();
    let in_a: Vec<i64> = core
        .vindex
        .search_in(&v, 5, Some(&[kb_a]))
        .iter()
        .map(|h| h.item_id)
        .collect();
    assert!(in_b.contains(&a));
    assert!(!in_a.contains(&a));
}

#[tokio::test]
async fn 同一文件可导入两个知识库_同库重复才算重复() {
    let (url, _m) = start_mock().await;
    let (core, dir) = new_core(&url);
    let kb_b = kb::create(&core, "B", "").unwrap().id;
    let f = dir.path().join("规划.md");
    std::fs::write(&f, "# 规划\n\n今年的目标是把检索做好").unwrap();
    let path = f.to_string_lossy().to_string();
    let cap = |kb: i64| {
        let core = core.clone();
        let path = path.clone();
        async move {
            ingest::capture(
                &core,
                CaptureInput {
                    paths: vec![path],
                    kb_id: Some(kb),
                    ..Default::default()
                },
            )
            .await
            .unwrap()
        }
    };
    let first = cap(DEFAULT_KB_ID).await;
    assert_eq!(first.created.len(), 1);
    let other_kb = cap(kb_b).await;
    assert_eq!(other_kb.created.len(), 1, "另一个知识库不算重复");
    assert!(other_kb.duplicates.is_empty());
    let same_kb = cap(kb_b).await;
    assert!(same_kb.created.is_empty());
    assert_eq!(same_kb.duplicates, other_kb.created, "同库才算重复");
}

#[tokio::test]
async fn 删除知识库_转移到默认库或级联删除() {
    let (core, _d, _kb_a, kb_b, _a, b) = two_kbs().await;
    // 转移到默认库
    let n = kb::delete(&core, kb_b, DeleteMode::MoveToDefault)
        .await
        .unwrap();
    assert_eq!(n, 1);
    let it = core.db.with(|c| repo::get_item(c, b)).unwrap().unwrap();
    assert_eq!(it.kb_id, DEFAULT_KB_ID);
    assert_eq!(core.db.with(repo::list_kbs).unwrap().len(), 1);

    // 级联删除:新建库放一条,删库后记录与向量一并消失
    let kb_c = kb::create(&core, "临时", "").unwrap().id;
    let c = put_in(&core, "临时库里的一条内容,马上就要被删除", kb_c).await;
    queue::drain_once(&core).await.unwrap();
    let before = core.vindex.len();
    kb::delete(&core, kb_c, DeleteMode::Cascade).await.unwrap();
    assert!(core.db.with(|cn| repo::get_item(cn, c)).unwrap().is_none());
    assert_eq!(core.vindex.len(), before - 1);
    // 默认库不可删
    assert!(kb::delete(&core, DEFAULT_KB_ID, DeleteMode::Cascade)
        .await
        .is_err());
}
