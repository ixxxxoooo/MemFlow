# OrbitBoard — 实时协作看板平台

> 本目录为 **OrbitBoard** 项目的示例文档集,用于 MemFlow 导入与 RAG 检索测试。
> @author ixxxxoooo

## 项目简介

OrbitBoard 是一个面向中小团队的实时协作看板(类似 Trello/Linear)。支持多看板、任务卡片、拖拽排序、评论、@ 提及、文件附件、回收站与权限控制。后端采用 Rust + Axum,前端为 React 19,实时同步基于 WebSocket + CRDT(automerge)。

## 核心特性

- 工作空间 → 项目 → 看板 → 列 → 卡片 的层级结构
- 实时协作:多用户同时编辑,光标、卡片拖拽状态同步
- 权限: Owner / Admin / Member / Guest 四级
- 全文搜索:PostgreSQL + pg_trgm + Meilisearch
- 文件附件: S3 兼容存储(MinIO / AWS S3)
- 审计日志与回收站(30 天软删除)
- Webhook + 对外 API(REST + GraphQL)

## 技术栈

| 层 | 技术 |
|---|---|
| 后端 | Rust 1.82, Axum 0.7, Tokio, SQLx, PostgreSQL 16 |
| 实时 | tokio-tungstenite, automerge 0.5 |
| 前端 | React 19, Vite, TypeScript 5.6, Tailwind 4 |
| 状态 | Zustand + TanStack Query |
| 缓存 | Redis 7(会话/限流/发布订阅) |
| 部署 | Docker Compose, Kubernetes(Helm), Terraform |

## 目录结构

```
orbitboard/
├── crates/
│   ├── core/          # 领域模型与业务规则
│   ├── api/           # Axum HTTP/WebSocket 路由
│   ├── db/            # SQLx 仓库实现
│   └── worker/        # 异步任务(邮件/通知/缩略图)
├── apps/
│   ├── web/           # React 前端
│   └── desktop/       # Tauri 封装(可选)
├── infra/             # Terraform + Helm
└── docs/              # 本目录
```

## 快速启动

```bash
docker compose up -d postgres redis minio meilisearch
cargo run -p api
pnpm --filter web dev
```

默认账号 `demo@orbitboard.dev / demo123`,访问 http://localhost:5173。

## 文档索引

- `docs/product-prd.md` 产品需求
- `docs/architecture.md` 架构设计
- `docs/api-overview.md` API 概览
- `docs/database-schema.md` 数据库表结构
- `docs/deployment.md` 部署指南
- `docs/authentication.md` 认证与授权
- `docs/frontend-guide.md` 前端开发指南
- `docs/backend-guide.md` 后端开发指南
- `docs/testing-strategy.md` 测试策略
- `docs/roadmap.md` 路线图
- `docs/adr/` 架构决策记录

---

# OrbitBoard — Real-time Collaborative Board Platform

OrbitBoard is a real-time collaborative kanban platform for small-to-medium teams. See above for Chinese details. All code and docs are sample fixtures for MemFlow ingestion testing.