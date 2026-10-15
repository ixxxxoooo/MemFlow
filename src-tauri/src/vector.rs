// @author ygw
//! 内存向量索引:向量以 BLOB 存于 SQLite,启动时载入内存,用暴力余弦检索。
//!
//! 个人知识库规模(十万切片以内)毫秒级返回,且不依赖任何 SQLite 扩展。
//! 向量统一做 L2 归一化,余弦相似度即点积。

use crate::db::Db;
use crate::error::Result;
use std::collections::HashMap;
use std::sync::RwLock;

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

/// 索引内部数据
#[derive(Default)]
struct Inner {
    /// 当前索引对应的嵌入模型
    model: String,
    /// item_id -> 该记录的所有切片向量
    items: HashMap<i64, Vec<Entry>>,
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
        let mut items: HashMap<i64, Vec<Entry>> = HashMap::new();
        let mut count = 0usize;
        db.with_read(|c| {
            let mut stmt = c.prepare(
                "SELECT id, item_id, embedding FROM chunks WHERE embed_model = ?1 AND embedding IS NOT NULL",
            )?;
            let rows = stmt.query_map([model], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, Vec<u8>>(2)?))
            })?;
            for row in rows {
                let (chunk_id, item_id, blob) = row?;
                let vec = decode(&blob);
                if is_valid(&vec) {
                    items.entry(item_id).or_default().push(Entry { chunk_id, vec });
                    count += 1;
                }
            }
            Ok(())
        })?;
        let mut g = self.inner.write().unwrap();
        g.model = model.to_string();
        g.items = items;
        Ok(count)
    }

    /// 替换某条记录的全部向量
    ///
    /// 参数: model 向量所属模型(与索引当前模型不一致则忽略);item_id 记录 id;entries 新向量
    pub fn replace_item(&self, model: &str, item_id: i64, entries: Vec<Entry>) {
        let mut g = self.inner.write().unwrap();
        if g.model != model {
            return;
        }
        if entries.is_empty() {
            g.items.remove(&item_id);
        } else {
            g.items.insert(item_id, entries);
        }
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
            .map(|v| v.len())
            .sum()
    }

    /// 索引是否为空
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 余弦检索
    ///
    /// 参数: query 查询向量(会在内部归一化副本);k 返回条数
    /// 返回: 按相似度降序的命中;维度不一致的向量被跳过
    pub fn search(&self, query: &[f32], k: usize) -> Vec<VecHit> {
        if !is_valid(query) || k == 0 {
            return vec![];
        }
        let mut q = query.to_vec();
        normalize(&mut q);
        let g = self.inner.read().unwrap();
        let mut hits: Vec<VecHit> = Vec::new();
        for (item_id, entries) in g.items.iter() {
            for e in entries {
                if e.vec.len() != q.len() {
                    continue;
                }
                let score: f32 = e.vec.iter().zip(q.iter()).map(|(a, b)| a * b).sum();
                hits.push(VecHit {
                    chunk_id: e.chunk_id,
                    item_id: *item_id,
                    score,
                });
            }
        }
        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        hits.truncate(k);
        hits
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

    #[test]
    fn 非法向量不检索() {
        let idx = VectorIndex::new();
        assert!(idx.search(&[f32::NAN, 1.0], 3).is_empty());
        assert!(idx.search(&[], 3).is_empty());
    }
}
