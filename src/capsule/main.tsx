// @author ygw
// 闪念胶囊入口

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { installErrorLog } from "../lib/errorLog";
import "../styles.css";
import { Capsule } from "./Capsule";

installErrorLog("capsule");

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Capsule />
  </StrictMode>,
);
