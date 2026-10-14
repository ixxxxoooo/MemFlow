# 认证与授权 — OrbitBoard

## 1. 认证方案

- 主认证:**邮箱 + 密码**,密码使用 Argon2id 哈希(cost memory 64MB, parallelism 2)
- 可选 **SSO**:Google/Microsoft/GitHub OAuth2 + OIDC
- 可选 **SAML 2.0**(enterprise plan)
- **2FA**:TOTP(Google Authenticator)+ 备用恢复码(10 个,一次性)

## 2. Token 设计

- access token:JWT RS256,15 分钟过期,含 `sub`(user id)、`sid`(session id)、`scopes`
- refresh token:opaque,30 天,HttpOnly+Secure+SameSite=Lax cookie,rotating(每次刷新换新)
- 设备会话:每设备一条 `sessions` 记录,支持远程登出

## 3. OAuth 流程(以 GitHub 为例)

1. 前端跳转 `/v1/auth/github/start?redirect=...`
2. 后端重定向 GitHub 授权页(state+PKCE)
3. 回调 `/v1/auth/github/callback` 校验 state,换取 token,拉取用户信息
4. 若邮箱已存在则绑定账号,否则创建用户
5. 签发 access+refresh,重定向回前端

## 4. 权限模型(RBAC)

### 工作空间角色

| 角色 | 管理成员 | 管理项目/看板 | 编辑卡片 | 只读 |
|---|---|---|---|---|
| Owner | ✓ | ✓ | ✓ | ✓ |
| Admin | ✓ | ✓ | ✓ | ✓ |
| Member | ✗ | ✗ | ✓ | ✓ |
| Guest | ✗ | ✗ | 仅被分配的卡片 | ✓ |

### 看板级覆盖

项目 Owner 可为单个看板设置"隐藏"(仅白名单成员可见)或"锁定"(只读)。

### 权限检查

后端通过 middleware `RequireRole` + 资源级 `Policy::can(user, action, resource)`。所有写操作审计落 `activities`。

## 5. 会话与安全

- 登录失败 5 次锁定 15 分钟(按 IP+邮箱)
- 密码重置链接 1 小时有效,单次使用
- 全设备登出:`DELETE /v1/sessions`(清空所有 refresh token)
- 敏感操作(删除工作空间、改 Owner)二次确认 + 密码

## 6. API Key(对外集成)

- 项目级/工作空间级 API key,scoped scopes(read/write/admin)
- header:`Authorization: Bearer ob_live_...`
- 支持过期时间与 IP 白名单
- 限流独立计数

## 7. 安全清单

- [ ] 密码最小 12 位,含大小写+数字
- [ ] 登录接口 CSRF + 速率限制
- [ ] refresh cookie Secure/HttpOnly/SameSite
- [ ] JWT 密钥轮换支持 multi-kid
- [ ] OAuth state + PKCE
- [ ] 2FA 恢复码哈希存储
- [ ] 会话列表展示设备/IP/最后活跃