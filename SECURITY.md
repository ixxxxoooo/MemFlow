# 安全策略

## 支持的版本

仅最新的发布版本会获得安全修复。

## 数据与隐私

MemFlow 为本地优先应用:所有记录、附件与向量保存在 `~/Library/Application Support/MemFlow/`,
模型推理通过本机 Ollama(默认 `http://127.0.0.1:11434`)完成,应用本身不上传任何用户数据。

## 报告漏洞

请**不要**在公开 Issue 中披露安全漏洞。请通过 GitHub 的
[Private vulnerability reporting](https://github.com/ixxxxoooo/MemFlow/security/advisories/new)
提交,并尽量包含复现步骤、影响范围与建议修复方式。

我们会在 7 个工作日内确认收到,并在修复发布后致谢报告者(如你愿意)。
