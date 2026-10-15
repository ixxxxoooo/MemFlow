// @author ixxxxoooo
// 主工作台入口

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { installErrorLog } from "../lib/errorLog";
import { applyPlatformAttr } from "../lib/platform";
import "../styles.css";
import { App } from "./App";

installErrorLog("main");
applyPlatformAttr();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
