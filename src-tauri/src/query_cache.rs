// @author ygw
//! 查询向量 LRU 缓存:同一问题(如重试、追问改写后相同)免去一次向量化往返。

use std::collections::{HashMap, VecDeque};

/// 容量固定的 LRU 缓存,键为 `模型名\0查询文本`
pub struct QueryCache {
    cap: usize,
    map: HashMap<String, Vec<f32>>,
    /// 最近使用在队尾
    order: VecDeque<String>,
}

impl QueryCache {
    /// 创建缓存
    ///
    /// 参数: cap 最大条目数(至少为 1)
    pub fn new(cap: usize) -> Self {
        Self {
            cap: cap.max(1),
            map: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn key(model: &str, query: &str) -> String {
        format!("{model}\0{query}")
    }

    /// 取缓存并标记为最近使用
    pub fn get(&mut self, model: &str, query: &str) -> Option<Vec<f32>> {
        let k = Self::key(model, query);
        let v = self.map.get(&k)?.clone();
        self.touch(&k);
        Some(v)
    }

    /// 写入缓存,超出容量时淘汰最久未用的条目
    pub fn put(&mut self, model: &str, query: &str, vec: Vec<f32>) {
        let k = Self::key(model, query);
        if self.map.insert(k.clone(), vec).is_some() {
            self.touch(&k);
            return;
        }
        self.order.push_back(k);
        while self.order.len() > self.cap {
            if let Some(old) = self.order.pop_front() {
                self.map.remove(&old);
            }
        }
    }

    fn touch(&mut self, k: &str) {
        if let Some(pos) = self.order.iter().position(|x| x == k) {
            if let Some(key) = self.order.remove(pos) {
                self.order.push_back(key);
            }
        }
    }

    /// 当前条目数
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 命中后刷新顺序并淘汰最久未用() {
        let mut c = QueryCache::new(2);
        c.put("m", "a", vec![1.0]);
        c.put("m", "b", vec![2.0]);
        assert_eq!(c.get("m", "a"), Some(vec![1.0])); // a 变为最近使用
        c.put("m", "c", vec![3.0]); // 淘汰 b
        assert!(c.get("m", "b").is_none());
        assert!(c.get("m", "a").is_some());
        assert!(c.get("m", "c").is_some());
        assert_eq!(c.len(), 2);
    }

    #[test]
    fn 不同模型的同一查询互不干扰() {
        let mut c = QueryCache::new(4);
        c.put("m1", "q", vec![1.0]);
        assert!(c.get("m2", "q").is_none());
    }
}
