// @author ygw
// 主工作台入口

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "../styles.css";
import { App } from "./App";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
