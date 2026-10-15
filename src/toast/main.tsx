// @author ixxxxoooo
// 屏幕右下角的轻量提示(由 Rust 控制显示与自动隐藏)

import { listen } from "@tauri-apps/api/event";
import { StrictMode, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { installErrorLog } from "../lib/errorLog";
import "../styles.css";

function Toast() {
  const [msg, setMsg] = useState("");
  const [tick, setTick] = useState(0);
  useEffect(() => {
    const p = listen<string>("toast-message", (e) => {
      setMsg(e.payload);
      setTick((t) => t + 1);
    });
    return () => { p.then((f) => f()); };
  }, []);
  return (
    <div className="flex h-full w-full items-center justify-center p-1.5">
      <div
        key={tick}
        className="pop-in rounded-full border border-[var(--line)] px-4 py-2 text-[13px] font-medium shadow-lg backdrop-blur-xl"
        style={{ background: "var(--glass)" }}
      >
        {msg}
      </div>
    </div>
  );
}

installErrorLog("toast");

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Toast />
  </StrictMode>,
);
