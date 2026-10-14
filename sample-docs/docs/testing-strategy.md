# 测试策略 — OrbitBoard

## 1. 测试金字塔

| 层 | 工具 | 覆盖目标 | 运行时机 |
|---|---|---|---|
| 单元 | Rust cargo test / Vitest | core 90%, web 70% | PR + main |
| 集成 | Testcontainers + Playwright | 关键路径 | PR |
| E2E | Playwright | 冒烟场景 20 条 | nightly + release |
| 性能 | k6 + Lighthouse | 基线 | weekly |
| 安全 | cargo audit, npm audit, OWASP ZAP | - | weekly |

## 2. 单元测试约定

- Rust:`#[cfg(test)]` 模块紧邻实现;core crate 必须 ≥ 90% 覆盖
- 前端:组件用 Testing Library,禁止 `getByTestId` 优先于语义查询
- 测试命名:`<unit>_<condition>_<expected>`

## 3. 集成测试

后端使用 Testcontainers:

```rust
#[sqlx::test(migrations = "./migrations")]
async fn create_card_then_move(pool: PgPool) {
    let repo = SqlxCardRepo::new(pool);
    let card = repo.create(...).await.unwrap();
    repo.move_to(...).await.unwrap();
    // 断言
}
```

前端 MSW mock API + RTL。

## 4. E2E 冒烟场景(Playwright)

1. 登录 → 创建工作空间 → 创建看板 → 添加列
2. 创建卡片 → 拖拽到另一列 → 刷新后顺序正确
3. 两浏览器标签页同时拖拽同一卡片 → 最终一致
4. 评论 @ 提及 → 通知中心出现
5. 附件上传 → 预览图显示
6. Guest 用户访问私有看板 → 403
7. 导出 CSV → 文件下载
8. GitHub PR webhook → 卡片显示 PR 状态
9. 搜索 "暗色模式" → 命中卡片
10. 回收站恢复已删除卡片

## 5. 性能测试(k6)

场景:50 并发用户对同一看板持续拖拽 5 分钟。指标:
- WS 消息延迟 p95 < 100ms
- API p95 < 300ms
- 错误率 < 0.1%
- 内存无泄漏

## 6. 回归策略

- 每个 bug 修复必须附带复现测试
- Flaky test  quarantine 列表,7 天内修复否则删除
- E2E 失败截图 + trace 自动上传 CI artifact

## 7. 覆盖率门禁

- core:≥ 90%
- db:≥ 70%
- api handlers:≥ 60%
- web components:≥ 60%
- 低于门禁阻断合并(Codecov status check)