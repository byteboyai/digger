// 正常运行:挂载编辑器,向 Rust 说 ready,文档由 Rust 在 open 里给出(页面不自带语料)。
import type { Engine } from "./engine";
import { post } from "./bridge";

export function baseStyle() {
  document.documentElement.dataset.theme = window.__DIGGER_THEME__ ?? "light";
  const style = document.createElement("style");
  style.textContent = `:root{--editor-bg:#fbfaf7;--editor-fg:#1d1d1b;--editor-code:#eee;--editor-link:#2a62c9;--editor-muted:#555;--editor-border:#bbb;--editor-selection:#cde0ff;color-scheme:light}
  :root[data-theme="dark"]{--editor-bg:#161820;--editor-fg:#e6e1d5;--editor-code:#292c36;--editor-link:#9bb8ff;--editor-muted:#b0abb5;--editor-border:#51545e;--editor-selection:#3c4965;color-scheme:dark}
  html,body{margin:0;height:100%;background:var(--editor-bg);color:var(--editor-fg)}
  #editor{height:100%;overflow:auto;font:17px/1.8 -apple-system,"PingFang SC",sans-serif}`;
  document.head.appendChild(style);
}

export function startApp(e: Engine) {
  baseStyle();
  e.mount(document.getElementById("editor")!);
  window.addEventListener("error", (ev) => post({ t: "info", name: "page error", detail: ev.message }));
  if (e.rust) e.rust.start(e.name);
  else post({ t: "ready", protocol: 2, engine: e.name });
}
