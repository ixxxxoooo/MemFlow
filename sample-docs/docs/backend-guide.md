# 后端开发指南 — OrbitBoard API (Rust)

## 1. crate 划分

```
crates/
├── core/      # 领域实体、错误类型、trait 定义、纯逻辑
├── db/        # SQLx 实现 core traits
├── api/       # Axum 路由、handlers、middleware、DTO
└── worker/    # 任务消费者
```

`Cargo.toml` workspace;`core` 无 I/O 依赖,方便测试。

## 2. Axum 入口

```rust
#[tokio::main]
async fn main() {
    let cfg = Config::from_env().validate().unwrap();
    let pool = PgPool::connect(&cfg.database_url).await.unwrap();
    let redis = redis::Client::open(cfg.redis_url).unwrap();
    let state = AppState { pool, redis, cfg };
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();
    axum::serve(listener, app).with_graceful_shutdown(shutdown_signal()).await.unwrap();
}
```

## 3. 路由组织

每个 feature 一个 module:`api/src/routes/cards.rs` 暴露 `pub fn router() -> Router<AppState>`。在 `build_router` 中 `nest("/v1", merge(...))`。

## 4. 中间件栈

1. TraceLayer(OpenTelemetry)
2. CorsLayer
3. CompressionLayer
4. RequestId
5. AuthLayer(解析 JWT / API key → CurrentUser)
6. RateLimitLayer(Redis 滑动窗口)

顺序:先 trace/cors,再 auth,最后 rate limit(按用户维度)。

## 5. 数据库访问

- SQLx,编译期检查查询(`cargo sqlx prepare` 生成 `.sqlx` 离线元数据,CI 使用)
- 一个请求一个事务;写操作显式 `pool.begin()`
- 仓库 trait 定义在 core,实现 db;handler 依赖 trait(dyn 或泛型)
- 迁移:`sqlx migrate run`,启动时自动执行(或独立 job)

## 6. 错误处理

`crates/core/src/error.rs` 定义 `AppError` enum,实现 `IntoResponse` 映射到 HTTP 状态 + JSON。handler 返回 `Result<Json<T>, AppError>`。

## 7. 实时 (WebSocket)

- 路由 `GET /v1/realtime` 升级 WebSocket
- 连接注册到 `BoardHub`(Arc<Mutex<HashMap<BoardId, Broadcast>>>)
- 上行 change 校验 + 广播;持久化由 debounce task 触发(5s 或 100 changes)
- 断连清理 presence

## 8. 后台任务

worker 消费 Redis Streams:`notifications`, `indexing`, `thumbnails`, `webhooks`。每种 stream 独立 consumer group,支持水平扩展;失败消息入 DLQ。

## 9. 测试

- 单元:`core` 逻辑 `cargo test`
- 集成:Testcontainers 启动 PG+Redis,`#[sqlx::test]` 跑迁移
- API:axum-test / reqwest against `app` router in-process
- 覆盖率目标 80%(core 90%)

## 10. 发布

- `cargo build --release`(strip + lto)
- Docker 镜像:`rust:1.82-slim` 构建 + `debian:bookworm-slim` 运行
- 健康检查:`GET /v1/healthz` → `{status:"ok",db:"ok",redis:"ok"}`