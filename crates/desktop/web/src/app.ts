// 正常运行:挂载编辑器,向 Rust 说 ready,文档由 Rust 在 open 里给出(页面不自带语料)。
import type { Engine } from "./engine";
import { post } from "./bridge";

export function baseStyle() {
  const style = document.createElement("style");
  style.textContent = `html,body{margin:0;height:100%;background:#fbfaf7;color:#1d1d1b}
  #editor{height:100%;overflow:auto;font:17px/1.8 -apple-system,"PingFang SC",sans-serif}`;
  document.head.appendChild(style);
}

export function startApp(e: Engine) {
  baseStyle();
  e.mount(document.getElementById("editor")!);
  window.addEventListener("error", (ev) => post({ t: "info", name: "page error", detail: ev.message }));
  post({ t: "ready", engine: e.name });
}
