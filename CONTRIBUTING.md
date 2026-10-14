# 贡献指南

感谢你对 MemFlow 的关注!

## 开发环境

- macOS 12+、Rust stable、Node.js ≥ 20、pnpm(版本见 `package.json` 的 `packageManager`)
- 本机运行 [Ollama](https://ollama.com) 仅在真机冒烟测试和手动体验时需要;单元/集成测试使用 mock Ollama,无需模型。

```bash
pnpm install
pnpm tauri dev
```

## 提交前检查

```bash
pnpm build                                   # tsc 类型检查 + vite 构建
pnpm test                                    # 前端测试
cd src-tauri
cargo fmt --all -- --check                   # 格式
cargo clippy --all-targets -- -D warnings    # 静态检查
cargo test                                   # 单元 + 集成测试
```

CI 会执行同样的检查,未通过的 PR 不会被合并。

## 代码规范

- 代码头部标注作者(`@author`),注释使用中文,方法需注明用途、参数、返回值。
- Rust:`cargo fmt` 统一格式;避免 `unwrap()` 出现在非测试路径;对外输入做合法性校验。
- 保持"原始数据先落盘、AI 处理异步可重试"的健壮性原则,任何衍生数据的写入都应在单事务内完成。
- 新功能需附带测试;修复缺陷请先补一个能复现的测试。

## 提交信息

使用 [Conventional Commits](https://www.conventionalcommits.org/zh-hans/),例如:

```
feat: 支持 docx 表格文本提取
fix: 修复胶囊在多显示器下的定位
docs: 补充 fake-ip 环境下的模型拉取说明
```

常用类型:`feat` `fix` `docs` `refactor` `test` `perf` `build` `ci` `chore`。

## 发版流程(维护者)

1. 同步修改 `package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json` 的版本号,更新 `CHANGELOG.md`。
2. 提交并推送到 `main`,等待 CI 通过。
3. 打 tag 并推送:`git tag -a vX.Y.Z -m "MemFlow vX.Y.Z" && git push origin vX.Y.Z`。
4. `Release` 工作流会自动构建 Apple Silicon 与 Intel 的 `.dmg`,生成 `SHA256SUMS.txt` 并发布 GitHub Release。

## 报告问题

请使用 Issue 模板,并附上 macOS 版本、芯片架构、MemFlow 版本与 Ollama 版本。安全相关问题请勿公开提 Issue,见 [SECURITY.md](SECURITY.md)。
