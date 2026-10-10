# md-engine spike

一次性验证 ADR-012 的编辑器引擎对比:同一份语料、同一套测试,跑在真实 WKWebView 里。随时可删。

```bash
cd host && npm install && npm run build      # 产物 host/dist-cm/ 与 host/dist-pm/(Rust 用 include_str! 嵌入)
cd .. && cargo build
ENGINE=cm MODE=test cargo run                # 自动:CodeMirror 6 实时预览
ENGINE=pm MODE=test cargo run                # 自动:ProseMirror 源码保真
ENGINE=cm cargo run                          # 手动:打开示例章节,试中日文输入法、渐进显示语法
ENGINE=pm cargo run
```

(本机 cargo 配置了 `build.target`,二进制在 `target/aarch64-apple-darwin/debug/`。)

- `host/src/corpus.ts` — 一份真实形态的章节:中文、setext 标题、`_`/`__` 强调、`1)` 列表、硬换行、引用式链接、脚注、表格、HTML 注释、图片。
- `host/src/engine.ts` — 两个引擎共同实现的接口;`harness.ts` 是同一套测试。
- `host/src/cm-engine.ts` — CodeMirror 6:文档就是 Markdown 文本,语法由装饰隐藏,光标所在的顶层块显示语法。
- `host/src/pm-engine.ts` — ProseMirror + 源码保真序列化:每个顶层块记住它的原文,未改动的块保存时原样输出(含原来的块间空白),只有改动的块经过规范化序列化;表格/HTML 作为原样块。
- `src/sync.rs`、`host/src/rust-sync.ts` — Rust 持有文本与块结构;编辑器把改动作为 UTF-16 文本补丁发给 Rust,Rust 重解析、对齐块 ID,回传带版本的块列表;在途期间编辑器把块范围按未确认的改动映射。测试 T11–T18。
- `host/src/footnote.ts` — 给 Lezer 补的脚注定义扩展(编辑器配置的一部分;`spike/parser-agreement` 复用同一份文件)。`host/scripts/make-fixtures.mjs` 用 Rust 解析器生成 `fixtures.ts`,测试 T10 在 webview 里用编辑器自己的解析器与之对照。
- 结果见 ADR-012「Engine spike results」与「Parser agreement results」。
