# 架构设计 — OrbitBoard

## 1. 总体架构

OrbitBoard 采用 **模块化单体 + 异步任务队列** 架构:核心业务在单个 Rust 二进制中运行,重任务(邮件、缩略图、全文索引)卸载到 worker 进程,通过 Redis Streams 解耦。

```
┌────────────┐  HTTPS/WS   ┌──────────────┐
│  Web (SPA) │ ◄──────────► │   API (Axum) │
└────────────┘              └──────┬───────┘
                                  │
        ┌─────────────────────────┼─────────────────────────┐
        │                         │                         │
   ┌────▼────┐              ┌────▼────┐              ┌────▼────┐
   │ Postgres│              │  Redis  │              │  MinIO  │
   └─────────┘              └────┬────┘              └─────────┘
                                 │
                          ┌──────▼──────┐
                          │   Worker(s)  │
                          └─────────────┘
```

## 2. 分层

- **core** crate:纯领域模型与业务规则,无 I/O,便于单元测试
- **db** crate:SQLx 仓库实现,trait 定义在 core
- **api** crate:Axum 路由、中间件、DTO 转换、WebSocket 处理
- **worker** crate:消费者,处理 Redis Streams 中的任务

依赖方向:api → core,db;worker → core,db。core 不依赖任何 I/O crate。

## 3. 实时协作协议

采用 **Automerge CRDT** 作为看板状态的权威表示:

1. 客户端本地维护 Automerge doc,用户操作生成 change
2. change 通过 WebSocket 上行到 API,API 广播给同看板所有连接
3. 服务端持久化:每 N 个 change 或空闲 5s 后将 doc 快照写入 Postgres(`board_snapshots`)
4. 新加入客户端拉取最新快照 + 增量 change 流

冲突解决由 CRDT 保证;服务端作为中继+持久化,不做业务合并。

### 消息类型

| type | 方向 | 说明 |
|---|---|---|
| `hello` | c→s | 携带 board_id、last_seq |
| `snapshot` | s→c | 全量 Automerge doc |
| `change` | 双向 | 增量 change(base64) |
| `presence` | 双向 | 光标/选区 |
| `ack` | s→c | 已持久化 seq |

## 4. 数据一致性

- 业务写操作:Postgres 事务内更新关系表 + 写入 outbox 事件(Redis Stream)
- 读:默认读主库;热点看板读副本 + 50ms 缓存(TanStack Query)
- 乐观锁:`cards.version` 字段,并发更新返回 409 Conflict

## 5. 缓存策略

| 数据 | 缓存 | TTL |
|---|---|---|
| 用户会话 | Redis | 15min 滑动 |
| 看板元数据 | Redis | 5min |
| 搜索结果 | Redis | 30s |
| 文件缩略图 | CDN/本地 | 永久 |

## 6. 部署拓扑

- **单机**:Docker Compose,所有组件在同一主机
- **集群**:Kubernetes + Helm,API 水平扩展(WebSocket 需 sticky session 或 Redis Pub/Sub 中继)
- **数据库**:主从 + PgBouncer 连接池;备份每日 pg_dump + WAL 归档

## 7. 可观测性

- 指标:Prometheus + Grafana(请求延迟、WS 连接数、队列深度)
- 日志:tracing → Loki/ELK(JSON 结构化)
- 链路:OpenTelemetry → Jaeger
- 告警:SLO 违约、队列积压、错误率

## 8. 安全边界

- 所有 API 必须 HTTPS;生产环境 HSTS
- JWT RS256,公钥发布于 `/v1/.well-known/jwks.json`
- CSRF:Doubled Submit Cookie + Origin 校验
- 输入校验:zod(前端)+ garde(后端)
- SQL:仅参数化查询,禁止字符串拼接
- 上传:类型白名单 + 大小限制 + 病毒扫描(Cloudmersive/自建)