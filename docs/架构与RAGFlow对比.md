# MemFlow 架构说明与 RAGFlow 对比

本文回答三个问题:数据存在哪里、MemFlow 与 RAGFlow 在各环节有什么不同、还有哪些性能手段。

## 1. 数据存放位置(macOS)

| 内容 | 位置 |
| --- | --- |
| 数据库(文档元数据、切片、向量、FTS 索引、任务队列、聊天记录、设置) | `~/Library/Application Support/MemFlow/memflow.db`(及 `-wal`、`-shm`) |
| 原始文件副本(导入的 pdf/docx/图片等) | `~/Library/Application Support/MemFlow/attachments/<hash 前 2 位>/` |
| 临时文件、数据库备份 | `.../MemFlow/tmp/`、`.../MemFlow/backups/` |
| 重排序模型 | `.../MemFlow/models/rerank/<模型名>/` |
| 运行日志(按天滚动,保留 7 天) | `~/Library/Logs/MemFlow/memflow.log.*` |
| 云端 API Key | macOS 钥匙串,服务名 `com.ixxxxoooo.memflow`(与 bundle 标识一致,旧版本的条目首次使用时自动迁移)(不入库、不进备份) |
| Ollama 模型 | `~/.ollama/models`(由 Ollama 管理) |

向量以 BLOB 存在 `chunks.embedding`,启动时按嵌入模型加载进内存做余弦检索;重建索引不需要重新向量化。

## 2. 与 RAGFlow 逐环节对比

| 环节 | RAGFlow | MemFlow |
| --- | --- | --- |
| 定位 | 多容器 Web 服务,面向团队、多知识库、大规模 | 单机桌面应用,面向个人隐私、低延迟、零运维 |
| 解析 | DeepDoc:版面分析 + OCR + 表格结构识别(ONNX,进程内);另可选 MinerU、Naive | PDFKit 文本层;扫描页走 macOS Vision OCR;表格用坐标聚类生成 Markdown;可选视觉大模型整页转 Markdown(高精度) |
| 切片 | 按模板(naive、paper、book、table、qa 等),默认约 512 token,可设分隔符与重叠;可附加关键词/问题/摘要;可选 RAPTOR、GraphRAG | 字符预算(默认 600 字、重叠 80),标题路径作前缀,表格独立成片并重复表头,保留页码;暂无 RAPTOR/GraphRAG |
| 元数据存储 | MySQL | SQLite(`items`、`item_meta`) |
| 原文件存储 | MinIO(或 S3/OSS/GCS) | 本地 `attachments/` 目录 |
| 缓存与队列 | Redis/Kvrocks(新版另有 NATS) | SQLite 持久化任务队列 + 进程内并发 worker |
| 切片/向量/全文索引 | Elasticsearch(或 Infinity、OpenSearch) | SQLite 表 + FTS5(jieba 分词)+ 内存向量索引 |
| 检索 | 关键词 + 向量按权重融合,可接 rerank 模型 | 向量 + FTS5 → RRF 融合 → 本地 ONNX 重排序(可选)→ 时效加权 → 相关度门槛 |
| 生成 | 多模型、可配置提示词/知识库参数 | 只依据检索资料回答,`[n]` 引用校验,无命中拒答;对话中可随时切换本地/云端模型 |

### Elasticsearch 在 RAGFlow 里干什么

它同时承担了五件事:

1. **BM25 倒排全文检索**:关键词、编号、人名等精确匹配。
2. **向量 kNN 检索**:语义相似召回。
3. **字段过滤**:按知识库、文档、标签等条件缩小范围。
4. **短语检索与高亮**:定位命中片段。
5. **水平扩展**:分片与副本,支撑百万到亿级切片。

MemFlow 的对应关系:BM25 → SQLite FTS5;kNN → 内存暴力余弦(个人规模万级切片、毫秒级);过滤 → SQL `WHERE`;高亮 → 前端处理。代价是规模上限:切片数达到几十万以上时,需要引入 ANN 索引(如 HNSW),RAGFlow 这类服务端方案在该规模下更合适。

### 优势与差距

- MemFlow 更强:启动即用、无外部依赖、数据不出本机、首字延迟低、增量处理与崩溃恢复内置。
- RAGFlow 更强:版面/表格解析成熟度、切片模板与语义增强(RAPTOR/GraphRAG)、多用户与权限、超大规模检索。

## 3. 性能设计与调优

已内置:

- SQLite WAL + 单写连接 + 多只读连接池,检索与入库互不阻塞;`synchronous=NORMAL`、64MB 页缓存、256MB mmap。
- 入库多 worker 并发,按阶段限流:提取/OCR 受 CPU 数限制,向量化并发 2,云端元数据并发 4、本地 1。
- 向量检索:入库时归一化,余弦退化为点积,`BinaryHeap` 取 top-k,切片较多时并行扫描。
- 对话:查询改写仅在需要时执行并与原问题检索并行(1.2 秒超时);模型预热与 `keep_alive`;查询向量 LRU 缓存。
- 界面:列表游标分页 + 虚拟滚动,状态轮询改事件推送。

建议的 Ollama 环境变量(终端 `launchctl setenv` 或 Ollama 菜单中配置后重启 Ollama):

```bash
OLLAMA_FLASH_ATTENTION=1        # 加速注意力计算
OLLAMA_KV_CACHE_TYPE=q8_0       # KV 缓存量化,显著省内存
OLLAMA_NUM_PARALLEL=2           # 允许并行请求(配合入库并发)
OLLAMA_KEEP_ALIVE=30m           # 模型常驻内存,避免首问冷启动
```

规模扩大后的路线:切片 > 20 万时引入 HNSW(如 usearch)、向量 int8 量化;需要多设备同步时再评估服务端存储。
