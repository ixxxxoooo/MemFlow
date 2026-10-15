// @author ixxxxoooo
//! 内存向量索引:向量以 BLOB 存于 SQLite,启动时载入内存,用暴力余弦检索。
//!
//! 个人知识库规模(十万切片以内)毫秒级返回,且不依赖任何 SQLite 扩展。
//! 向量统一做 L2 归一化,余弦相似度即点积。
//!
//! 内存布局:每条记录的全部切片向量平铺在一块连续的 `Vec<f32>` 里(逐片步长为维度),
//! 避免每个切片一次堆分配与指针跳转;检索用大小为 k 的小顶堆取 top-k(无需全量排序),
//! 条目超过 `PARALLEL_THRESHOLD` 时用 rayon 并行扫描。

use crate::db::Db;
use crate::error::Result;
use rayon::prelude::*;
use std::cmp::{Ordering, Reverse};
use std::collections::{BinaryHeap, HashMap};
use std::sync::RwLock;

/// 超过该条目数时并行扫描
const PARALLEL_THRESHOLD: usize = 20_000;

/// 向量条目
#[derive(Clone)]
pub struct Entry {
    /// 切片 id
    pub chunk_id: i64,
    /// 归一化后的向量
    pub vec: Vec<f32>,
}

/// 检索命中
#[derive(Debug, Clone, PartialEq)]
pub struct VecHit {
    /// 切片 id
    pub chunk_id: i64,
    /// 所属记录 id
    pub item_id: i64,
    /// 余弦相似度
    pub score: f32,
}

/// 一条记录的全部切片向量(平铺存储)
struct Block {
    /// 所属知识库(转移记录时只改这里,不必重新嵌入)
    kb_id: i64,
    /// 向量维度
    dim: usize,
    /// 各切片 id,与 `data` 按序对应
    chunk_ids: Vec<i64>,
    /// 平铺的向量数据,长度 = chunk_ids.len() * dim
    data: Vec<f32>,
}

impl Block {
    /// 由条目构建;以第一个合法条目的维度为准,维度不同的条目被丢弃
    fn from_entries(kb_id: i64, entries: Vec<Entry>) -> Option<Self> {
        let dim = entries.first()?.vec.len();
        if dim == 0 {
            return None;
        }
        let mut chunk_ids = Vec::with_capacity(entries.len());
        let mut data = Vec::with_capacity(entries.len() * dim);
        for e in entries {
            if e.vec.len() != dim {
                continue;
            }
            chunk_ids.push(e.chunk_id);
            data.extend_from_slice(&e.vec);
        }
        Some(Self {
            kb_id,
            dim,
            chunk_ids,
            data,
        })
    }
}

/// 堆中的候选:按得分排序,得分相同按 chunk_id 保证结果稳定
#[derive(Clone, Copy)]
struct Cand {
    score: f32,
    chunk_id: i64,
    item_id: i64,
}

impl PartialEq for Cand {
    fn eq(&self, o: &Self) -> bool {
        self.cmp(o) == Ordering::Equal
    }
}
impl Eq for Cand {}
impl PartialOrd for Cand {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Cand {
    fn cmp(&self, o: &Self) -> Ordering {
        self.score
            .total_cmp(&o.score)
            .then_with(|| o.chunk_id.cmp(&self.chunk_id))
    }
}

/// 维护大小不超过 k 的小顶堆(堆顶是当前最差的候选)
fn push_top_k(heap: &mut BinaryHeap<Reverse<Cand>>, c: Cand, k: usize) {
    if heap.len() < k {
        heap.push(Reverse(c));
    } else if heap.peek().is_some_and(|worst| c > worst.0) {
        heap.pop();
        heap.push(Reverse(c));
    }
}

/// 扫描一块记录,把候选并入堆
fn scan_block(heap: &mut BinaryHeap<Reverse<Cand>>, item_id: i64, b: &Block, q: &[f32], k: usize) {
    if b.dim != q.len() {
        return;
    }
    for (i, row) in b.data.chunks_exact(b.dim).enumerate() {
        let score: f32 = row.iter().zip(q).map(|(a, x)| a * x).sum();
        push_top_k(
            heap,
            Cand {
                score,
                chunk_id: b.chunk_ids[i],
                item_id,
            },
            k,
        );
    }
}

/// 索引内部数据
#[derive(Default)]
struct Inner {
    /// 当前索引对应的嵌入模型
    model: String,
    /// item_id -> 该记录的所有切片向量
    items: HashMap<i64, Block>,
}

/// 向量索引(线程安全)
#[derive(Default)]
pub struct VectorIndex {
    inner: RwLock<Inner>,
}

/// 把 f32 向量编码为小端字节
///
/// 参数: v 向量
/// 返回: 字节序列
pub fn encode(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for x in v {
        out.extend_from_slice(&x.to_le_bytes());
    }
    out
}

/// 把小端字节解码为 f32 向量
///
/// 参数: b 字节序列
/// 返回: 向量(长度不是 4 的倍数时丢弃尾部)
pub fn decode(b: &[u8]) -> Vec<f32> {
    // as_chunks 自动丢弃长度不足 4 的尾部
    b.as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect()
}

/// 原地 L2 归一化;零向量保持不变
///
/// 参数: v 向量
pub fn normalize(v: &mut [f32]) {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 1e-12 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

/// 向量是否合法(非空且全部为有限数)
pub fn is_valid(v: &[f32]) -> bool {
    !v.is_empty() && v.iter().all(|x| x.is_finite())
}

impl VectorIndex {
    /// 创建空索引
    pub fn new() -> Self {
        Self::default()
    }

    /// 从数据库载入指定嵌入模型的全部向量(会清空旧内容)
    ///
    /// 参数: db 数据库;model 当前嵌入模型名
    /// 返回: 载入的向量条数
    pub fn load_from_db(&self, db: &Db, model: &str) -> Result<usize> {
        let mut grouped: HashMap<i64, (i64, Vec<Entry>)> = HashMap::new();
        let mut count = 0usize;
        db.with_read(|c| {
            let mut stmt = c.prepare(
                "SELECT c.id, c.item_id, c.embedding, i.kb_id FROM chunks c JOIN items i ON i.id = c.item_id \
                 WHERE c.embed_model = ?1 AND c.embedding IS NOT NULL",
            )?;
            let rows = stmt.query_map([model], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            })?;
            for row in rows {
                let (chunk_id, item_id, blob, kb_id) = row?;
                let vec = decode(&blob);
                if is_valid(&vec) {
                    grouped
                        .entry(item_id)
                        .or_insert_with(|| (kb_id, Vec::new()))
                        .1
                        .push(Entry { chunk_id, vec });
                    count += 1;
                }
            }
            Ok(())
        })?;
        let items: HashMap<i64, Block> = grouped
            .into_iter()
            .filter_map(|(id, (kb, es))| Block::from_entries(kb, es).map(|b| (id, b)))
            .collect();
        let mut g = self.inner.write().unwrap();
        g.model = model.to_string();
        g.items = items;
        Ok(count)
    }

    /// 替换某条记录的全部向量
    ///
    /// 参数: model 向量所属模型(与索引当前模型不一致则忽略);item_id 记录 id;
    /// kb_id 记录所属知识库;entries 新向量
    pub fn replace_item_in(&self, model: &str, item_id: i64, kb_id: i64, entries: Vec<Entry>) {
        let mut g = self.inner.write().unwrap();
        if g.model != model {
            return;
        }
        match Block::from_entries(kb_id, entries) {
            Some(b) => {
                g.items.insert(item_id, b);
            }
            None => {
                g.items.remove(&item_id);
            }
        }
    }

    /// 替换某条记录的全部向量(归属默认知识库,仅测试使用)
    #[cfg(test)]
    pub fn replace_item(&self, model: &str, item_id: i64, entries: Vec<Entry>) {
        self.replace_item_in(model, item_id, crate::db::DEFAULT_KB_ID, entries);
    }

    /// 修改一批记录所属的知识库(转移记录时调用,不需要重新嵌入)
    pub fn set_items_kb(&self, item_ids: &[i64], kb_id: i64) {
        let mut g = self.inner.write().unwrap();
        for id in item_ids {
            if let Some(b) = g.items.get_mut(id) {
                b.kb_id = kb_id;
            }
        }
    }

    /// 取一批记录的"中心向量"(各切片向量的平均再归一化),用于判断记录间的整体相似度
    ///
    /// 参数: item_ids 记录 id
    /// 返回: id -> (知识库 id, 归一化中心向量);不在索引里的记录不返回
    pub fn centroids_of(&self, item_ids: &[i64]) -> HashMap<i64, (i64, Vec<f32>)> {
        let g = self.inner.read().unwrap();
        let mut out = HashMap::new();
        for id in item_ids {
            let Some(b) = g.items.get(id) else { continue };
            let mut acc = vec![0f32; b.dim];
            for row in b.data.chunks_exact(b.dim) {
                for (a, x) in acc.iter_mut().zip(row) {
                    *a += x;
                }
            }
            normalize(&mut acc);
            if is_valid(&acc) {
                out.insert(*id, (b.kb_id, acc));
            }
        }
        out
    }

    /// 移除某条记录
    pub fn remove_item(&self, item_id: i64) {
        self.inner.write().unwrap().items.remove(&item_id);
    }

    /// 清空索引并切换模型
    pub fn reset(&self, model: &str) {
        let mut g = self.inner.write().unwrap();
        g.model = model.to_string();
        g.items.clear();
    }

    /// 当前索引的向量总数
    pub fn len(&self) -> usize {
        self.inner
            .read()
            .unwrap()
            .items
            .values()
            .map(|b| b.chunk_ids.len())
            .sum()
    }

    /// 索引是否为空
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 余弦检索(全部知识库)
    ///
    /// 参数: query 查询向量(会在内部归一化副本);k 返回条数
    /// 返回: 按相似度降序的命中;维度不一致的向量被跳过
    pub fn search(&self, query: &[f32], k: usize) -> Vec<VecHit> {
        self.search_in(query, k, None)
    }

    /// 余弦检索,可限定知识库范围
    ///
    /// 参数: query 查询向量;k 返回条数;kb_filter 限定的知识库(None 或空 = 不限制)
    /// 返回: 按相似度降序的命中;范围外的记录直接跳过,不参与排序
    pub fn search_in(&self, query: &[f32], k: usize, kb_filter: Option<&[i64]>) -> Vec<VecHit> {
        let kb_filter = kb_filter.filter(|f| !f.is_empty());
        if !is_valid(query) || k == 0 {
            return vec![];
        }
        let mut q = query.to_vec();
        normalize(&mut q);
        let g = self.inner.read().unwrap();
        let in_scope = |b: &Block| kb_filter.is_none_or(|f| f.contains(&b.kb_id));
        let total: usize = g
            .items
            .values()
            .filter(|b| in_scope(b))
            .map(|b| b.chunk_ids.len())
            .sum();

        let heap = if total >= PARALLEL_THRESHOLD {
            // 并行:每个线程维护本地小顶堆,最后合并
            let blocks: Vec<(&i64, &Block)> = g.items.iter().filter(|(_, b)| in_scope(b)).collect();
            blocks
                .par_iter()
                .fold(BinaryHeap::new, |mut h, (id, b)| {
                    scan_block(&mut h, **id, b, &q, k);
                    h
                })
                .reduce(BinaryHeap::new, |mut a, b| {
                    for Reverse(c) in b {
                        push_top_k(&mut a, c, k);
                    }
                    a
                })
        } else {
            let mut h = BinaryHeap::new();
            for (id, b) in g.items.iter().filter(|(_, b)| in_scope(b)) {
                scan_block(&mut h, *id, b, &q, k);
            }
            h
        };

        // 小顶堆 -> 按得分降序
        let mut out: Vec<Cand> = heap.into_iter().map(|Reverse(c)| c).collect();
        out.sort_by(|a, b| b.cmp(a));
        out.into_iter()
            .map(|c| VecHit {
                chunk_id: c.chunk_id,
                item_id: c.item_id,
                score: c.score,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(v: &[f32]) -> Vec<f32> {
        let mut x = v.to_vec();
        normalize(&mut x);
        x
    }

    #[test]
    fn 编码解码互逆() {
        let v = vec![0.5f32, -1.25, 3.0];
        assert_eq!(decode(&encode(&v)), v);
    }

    #[test]
    fn 检索按相似度排序() {
        let idx = VectorIndex::new();
        idx.reset("m");
        idx.replace_item(
            "m",
            1,
            vec![Entry {
                chunk_id: 10,
                vec: unit(&[1.0, 0.0]),
            }],
        );
        idx.replace_item(
            "m",
            2,
            vec![Entry {
                chunk_id: 20,
                vec: unit(&[0.7, 0.7]),
            }],
        );
        idx.replace_item(
            "m",
            3,
            vec![Entry {
                chunk_id: 30,
                vec: unit(&[0.0, 1.0]),
            }],
        );
        let hits = idx.search(&[1.0, 0.1], 2);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].chunk_id, 10);
        assert_eq!(hits[1].chunk_id, 20);
    }

    #[test]
    fn 维度不一致的向量被跳过() {
        let idx = VectorIndex::new();
        idx.reset("m");
        idx.replace_item(
            "m",
            1,
            vec![Entry {
                chunk_id: 1,
                vec: unit(&[1.0, 0.0, 0.0]),
            }],
        );
        assert!(idx.search(&[1.0, 0.0], 5).is_empty());
    }

    #[test]
    fn 模型不一致的写入被忽略() {
        let idx = VectorIndex::new();
        idx.reset("new-model");
        idx.replace_item(
            "old-model",
            1,
            vec![Entry {
                chunk_id: 1,
                vec: unit(&[1.0, 0.0]),
            }],
        );
        assert!(idx.is_empty());
    }

    /// 基准:暴力检索在不同规模下的耗时(1024 维,同 bge-m3)。
    /// 运行: cargo test --release bench_暴力检索 -- --ignored --nocapture
    #[test]
    #[ignore = "性能基准,手动运行"]
    fn bench_暴力检索() {
        for n in [10_000usize, 50_000, 100_000] {
            let idx = VectorIndex::new();
            idx.reset("m");
            // 简单线性同余生成伪随机向量,避免引入随机数依赖
            let mut seed: u32 = 12345;
            let mut next = || {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                (seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5
            };
            for i in 0..n {
                let v: Vec<f32> = (0..1024).map(|_| next()).collect();
                idx.replace_item(
                    "m",
                    i as i64,
                    vec![Entry {
                        chunk_id: i as i64,
                        vec: unit(&v),
                    }],
                );
            }
            let q: Vec<f32> = (0..1024).map(|_| next()).collect();
            let runs = 5;
            let t = std::time::Instant::now();
            for _ in 0..runs {
                std::hint::black_box(idx.search(&q, 30));
            }
            let ms = t.elapsed().as_secs_f64() * 1000.0 / runs as f64;
            let mb = (n * 1024 * 4) as f64 / 1024.0 / 1024.0;
            println!("N={n:>7}  单次检索 {ms:>7.1} ms  向量内存约 {mb:>6.0} MB");
        }
    }

    /// 朴素全量排序实现,作为 top-k 堆与并行路径的对照
    fn naive(idx: &VectorIndex, q: &[f32], k: usize) -> Vec<(i64, f32)> {
        let mut qn = q.to_vec();
        normalize(&mut qn);
        let g = idx.inner.read().unwrap();
        let mut all: Vec<(i64, f32)> = Vec::new();
        for b in g.items.values() {
            for (i, row) in b.data.chunks_exact(b.dim).enumerate() {
                all.push((
                    b.chunk_ids[i],
                    row.iter().zip(&qn).map(|(a, x)| a * x).sum(),
                ));
            }
        }
        all.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        all.truncate(k);
        all
    }

    fn fill(idx: &VectorIndex, n: usize, dim: usize) {
        idx.reset("m");
        let mut seed: u32 = 777;
        let mut next = || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5
        };
        for i in 0..n {
            // 每条记录 2 个切片,覆盖多切片平铺路径
            let es: Vec<Entry> = (0..2)
                .map(|j| Entry {
                    chunk_id: (i * 2 + j) as i64,
                    vec: unit(&(0..dim).map(|_| next()).collect::<Vec<_>>()),
                })
                .collect();
            idx.replace_item("m", i as i64, es);
        }
    }

    #[test]
    fn 堆取topk与全量排序结果一致() {
        let idx = VectorIndex::new();
        fill(&idx, 500, 16);
        let q: Vec<f32> = (0..16).map(|i| (i as f32).sin()).collect();
        let got: Vec<(i64, f32)> = idx
            .search(&q, 25)
            .iter()
            .map(|h| (h.chunk_id, h.score))
            .collect();
        let want = naive(&idx, &q, 25);
        assert_eq!(got.len(), 25);
        for (g, w) in got.iter().zip(&want) {
            assert_eq!(g.0, w.0);
            assert!((g.1 - w.1).abs() < 1e-6);
        }
    }

    #[test]
    fn 超过阈值时并行路径结果一致() {
        let idx = VectorIndex::new();
        // 11_000 条记录 x 2 切片 = 22_000 > 阈值
        fill(&idx, 11_000, 8);
        assert!(idx.len() >= PARALLEL_THRESHOLD);
        let q: Vec<f32> = (0..8).map(|i| (i as f32 * 0.7).cos()).collect();
        let got: Vec<i64> = idx.search(&q, 30).iter().map(|h| h.chunk_id).collect();
        let want: Vec<i64> = naive(&idx, &q, 30).iter().map(|x| x.0).collect();
        assert_eq!(got, want);
    }

    #[test]
    fn 替换与删除记录后检索同步() {
        let idx = VectorIndex::new();
        idx.reset("m");
        idx.replace_item(
            "m",
            1,
            vec![Entry {
                chunk_id: 1,
                vec: unit(&[1.0, 0.0]),
            }],
        );
        idx.replace_item(
            "m",
            1,
            vec![Entry {
                chunk_id: 2,
                vec: unit(&[1.0, 0.0]),
            }],
        );
        assert_eq!(idx.search(&[1.0, 0.0], 5)[0].chunk_id, 2);
        idx.remove_item(1);
        assert!(idx.is_empty());
    }

    #[test]
    fn 非法向量不检索() {
        let idx = VectorIndex::new();
        assert!(idx.search(&[f32::NAN, 1.0], 3).is_empty());
        assert!(idx.search(&[], 3).is_empty());
    }

    #[test]
    fn 知识库范围过滤只返回范围内命中() {
        let idx = VectorIndex::new();
        idx.reset("m");
        // 记录 1 在库 1,记录 2 在库 2;向量方向相同
        idx.replace_item_in(
            "m",
            1,
            1,
            vec![Entry {
                chunk_id: 10,
                vec: unit(&[1.0, 0.0]),
            }],
        );
        idx.replace_item_in(
            "m",
            2,
            2,
            vec![Entry {
                chunk_id: 20,
                vec: unit(&[1.0, 0.0]),
            }],
        );
        let ids = |h: Vec<VecHit>| -> Vec<i64> {
            let mut v: Vec<i64> = h.iter().map(|x| x.item_id).collect();
            v.sort();
            v
        };
        assert_eq!(ids(idx.search_in(&[1.0, 0.0], 5, Some(&[1]))), vec![1]);
        assert_eq!(ids(idx.search_in(&[1.0, 0.0], 5, Some(&[2]))), vec![2]);
        assert_eq!(
            ids(idx.search_in(&[1.0, 0.0], 5, Some(&[1, 2]))),
            vec![1, 2]
        );
        // 不限制或空范围 = 全部
        assert_eq!(ids(idx.search_in(&[1.0, 0.0], 5, None)), vec![1, 2]);
        assert_eq!(ids(idx.search_in(&[1.0, 0.0], 5, Some(&[]))), vec![1, 2]);
        // 范围内没有任何记录
        assert!(idx.search_in(&[1.0, 0.0], 5, Some(&[99])).is_empty());
    }

    #[test]
    fn 转移知识库只改归属不丢向量() {
        let idx = VectorIndex::new();
        idx.reset("m");
        idx.replace_item_in(
            "m",
            1,
            1,
            vec![Entry {
                chunk_id: 10,
                vec: unit(&[1.0, 0.0]),
            }],
        );
        assert!(idx.search_in(&[1.0, 0.0], 5, Some(&[2])).is_empty());
        idx.set_items_kb(&[1], 2);
        assert_eq!(idx.search_in(&[1.0, 0.0], 5, Some(&[2])).len(), 1);
        assert!(idx.search_in(&[1.0, 0.0], 5, Some(&[1])).is_empty());
        assert_eq!(idx.len(), 1);
    }
}
