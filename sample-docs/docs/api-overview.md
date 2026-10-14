# API 概览 — OrbitBoard

Base URL:`https://api.orbitboard.dev/v1`(生产) / `http://localhost:8080/v1`(本地)。

## 1. 认证

- `POST /v1/auth/register` `{email,name,password}`
- `POST /v1/auth/login` `{email,password}` → `{accessToken, user}` + Set-Cookie refresh
- `POST /v1/auth/refresh` → 新 accessToken
- `POST /v1/auth/logout`
- `POST /v1/auth/forgot-password` / `POST /v1/auth/reset-password`

accessToken 有效期 15 分钟,refresh 30 天(HttpOnly cookie)。

## 2. 通用约定

- 请求/响应 JSON;时间 ISO8601 UTC
- 分页:cursor-based `?cursor=<opaque>&limit=20`;响应含 `nextCursor`
- 幂等:`Idempotency-Key` header 支持 POST 重试
- 错误格式:

```json
{ "error": { "code": "card.not_found", "message": "...", "details": {} } }
```

HTTP 状态:400 校验失败,401 未认证,403 权限不足,404 不存在,409 冲突,422 语义错误,429 限流,500 服务端。

## 3. 资源路由

```
/v1/workspaces
  GET    /                 列表
  POST   /                 创建
  GET    /{id}
  PATCH  /{id}
  DELETE /{id}
  /members
    GET    /
    POST   /                邀请
    PATCH  /{userId}        改角色
    DELETE /{userId}        移除
  /projects
    ...

/v1/projects/{id}
/v1/projects/{id}/boards
/v1/boards/{id}
/v1/boards/{id}/columns
/v1/cards/{id}
/v1/cards/{id}/comments
/v1/cards/{id}/attachments
/v1/cards/{id}/activities
/v1/labels
/v1/search?q=&workspaceId=&...
/v1/users/me
/v1/notifications
/v1/webhooks
```

## 4. 卡片接口示例

### 创建卡片

`POST /v1/cards`

```json
{
  "boardId": "uuid",
  "columnId": "uuid",
  "title": "支持暗色模式",
  "description": "## 背景\n...",
  "assigneeId": "uuid",
  "priority": "high",
  "dueAt": "2026-11-01T00:00:00Z",
  "labelIds": ["uuid"]
}
```

响应 `201`:

```json
{
  "id": "uuid",
  "number": 124,
  "ref": "ORB-124",
  "title": "支持暗色模式",
  "position": "a0",
  "version": 1,
  "createdAt": "..."
}
```

### 移动卡片

`POST /v1/cards/{id}/move`

```json
{ "columnId": "uuid", "afterCardId": "uuid-or-null", "version": 3 }
```

返回更新后的卡片;版本不匹配返回 409。

## 5. 实时 WebSocket

`wss://api.../v1/realtime?token=<access>`

连接后发送 `{type:"subscribe", boardId}`;服务端推送快照与变更。客户端发送 change 上行。详见 `docs/architecture.md` §3。

## 6. GraphQL(实验性)

`POST /v1/graphql` 提供查询, mutation 仍建议用 REST(幂等/审计更清晰)。

## 7. 限流

- 匿名:60 req/min/IP
- 认证用户:600 req/min/user
- WebSocket 消息:100 msg/s/conn
- 超限返回 429 + `Retry-After` header

## 8. SDK

- JS/TS:`@orbitboard/sdk`(REST + WS 封装)
- Rust:`orbitboard-client` crate
- Python:`orbitboard` PyPI 包(社区维护)