// @author ixxxxoooo
// 测试环境统一模拟 macOS(项目文案以 ⌘ 为准);平台相关逻辑在 platform.test.ts 里显式传参验证
Object.defineProperty(window.navigator, "userAgent", {
  value: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)",
  configurable: true,
});
