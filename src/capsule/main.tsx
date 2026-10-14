// @author ygw
// 闪念胶囊入口

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "../styles.css";
import { Capsule } from "./Capsule";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Capsule />
  </StrictMode>,
);
