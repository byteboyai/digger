# editor-host spike

一次性验证 ADR-001 的编辑器宿主边界(验证项 1、2 的一部分)。随时可删。

## 运行

```bash
cd host && npm install && npm run build      # 产物 host/dist/index.html,由 Rust include_str! 嵌入
cd .. && cargo test                           # 桥接校验 + AST 投影/diff 单测
cargo run                                     # 手工:窗口左侧是 iced 面板,右侧是 Tiptap 宿主
SELFTEST=1 cargo run                          # 自动:宿主执行脚本化编辑,Rust 校验后打印 PASS/FAIL
```

(本机 cargo 配置了 `build.target`,二进制在 `target/aarch64-apple-darwin/debug/`。)

## 结构

- `src/bridge.rs` — `HostEnvelope`、入站校验(绑定、封闭白名单、大小上限、seq 严格递增)。
- `src/doc.rs` — 极简 AST(段落/标题)、AST→ProseMirror 投影、候选文档解析(拒缺失/重复 blockId、不支持节点)、语义操作 diff。
- `src/main.rs` — iced 窗口;`window::run` 里 `build_as_child` 建 wry 子视图;`evaluate_script` 下行,`ipc_handler` 经 channel 上行;`resize_events` 同步子视图 bounds。
- `host/src/main.ts` — Tiptap 宿主:`blockId` 全局属性 + `appendTransaction` 补/去重 id;单飞行编辑 + 提交确认;防抖快照;JS 错误经 `failed` 回传。

## 结果

见 ADR-001「Spike 结果」一节。
