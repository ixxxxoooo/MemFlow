# 前端开发指南 — OrbitBoard Web

## 1. 技术栈

- React 19 + TypeScript 5.6(strict)
- Vite 5, Tailwind CSS 4
- 路由:React Router 7
- 状态:Zustand(客户端) + TanStack Query(服务端)
- 表单:react-hook-form + zod
- 实时:自研 WS 客户端 + automerge
- 拖拽:`@dnd-kit/core`
- 测试:Vitest + Testing Library + Playwright E2E

## 2. 目录结构

```
apps/web/src/
├── app/             # 路由、布局、providers
├── features/
│   ├── boards/      # 看板 feature(组件/hook/api/types)
│   ├── cards/
│   ├── auth/
│   └── ...
├── components/ui/   # 通用组件(Button,Dialog,...)
├── lib/             # api client, ws, automerge 封装
└── styles/
```

feature 内部按 `components hooks api types` 划分,避免跨 feature 直接导入内部文件(通过 index.ts 暴露)。

## 3. 状态管理约定

- **客户端 UI 状态**(模态开关、选中卡片):Zustand slice,放在 feature 内
- **服务端数据**:TanStack Query;mutation 使用 `onMutate` 乐观更新 + `onError` 回滚
- 禁止在组件中直接 fetch;统一走 `lib/api`

## 4. 实时协作客户端

`lib/realtime.ts` 维护 WS 连接:

- 自动重连(指数退避,最大 30s)
- 心跳 ping 每 25s
- 订阅 board 时拉 snapshot,合并本地 doc
- presence 广播节流 50ms

Automerge change 应用后触发 Zustand/board store 更新 → React 重渲染;使用 `useMemo` 避免大列表重渲染。

## 5. 拖拽交互

- `@dnd-kit` + `SortableContext`
- 乐观更新:拖拽开始立即移动卡片(DOM),上行 change;失败回滚 + toast
- 可访问性:支持键盘 (空格 picked,方向键移动,Enter 放下)

## 6. 性能

- 虚拟列表:`@tanstack/react-virtual` 用于 > 50 卡片的列
- 图片:`<img loading="lazy">` + srcset;附件预览用 IntersectionObserver
- 代码分割:路由级 `React.lazy`
- Bundle 目标:首屏 JS < 200KB gzip

## 7. 样式

- Tailwind 4, design tokens 在 `styles/tokens.css`
- 暗色模式:`.dark` class on html,跟随系统 + 手动切换
- 字体:Inter Variable + JetBrains Mono(代码)

## 8. 本地开发

```bash
pnpm install
pnpm dev          # http://localhost:5173
pnpm test         # vitest
pnpm e2e          # playwright
pnpm lint
```

环境变量见 `.env.example`(`VITE_API_URL`,`VITE_WS_URL`)。