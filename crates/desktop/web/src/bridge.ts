// 页面 → Rust 的唯一出口:每条消息都带宿主注入的 nonce,没有 nonce 的消息 Rust 一律拒绝(ADR-013 §3 / §5)。
declare global {
  interface Window {
    ipc: { postMessage(m: string): void };
    /** 宿主的初始化脚本在页面脚本之前注入 */
    __DIGGER__?: { readonly nonce: string; readonly mode: "app" | "selftest" };
  }
}

export const post = (o: unknown) => window.ipc.postMessage(JSON.stringify({ n: window.__DIGGER__?.nonce ?? "", ...(o as object) }));
