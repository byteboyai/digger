# block-identity spike

一次性验证 ADR-012 §2:Markdown 文本是权威内容时,块 ID 如何在每次提交后保持稳定。纯 Rust,无界面。随时可删。

```bash
cd spike/block-identity
cargo test --release                      # 13 个单元测试 + 11 个属性测试(约 1–2 分钟)
cargo run --release --bin report 24       # 各场景的保留/丢失/错配表(24 篇文档 × 40 次提交)
cargo run --release --bin report 12 sweep # 相似度阈值扫描
cargo run --release --example timing      # 500–20000 块的对齐耗时
DIAG=1 cargo run --release --bin report   # 遇到"几乎没变却丢失"的块就打印完整诊断并退出
```

(本机 cargo 配置了 `build.target`,`rust-objcopy` 的 SIGABRT 警告是本机工具链问题,与结果无关。)

- `src/parse.rs` — 用 pulldown-cmark(CommonMark + GFM 表格、脚注、删除线、任务列表)取顶层块及其源码区间。
- `src/reconcile.rs` — 对齐算法:精确哈希(沿最长公共子序列)→ 乱序精确哈希(移动)→ 相同块按相邻块上下文分配 → 锚点之间的拆分/合并检测与单调相似度对齐 → 全局相似度配对(既改又移动)→ 新 ID。
- `src/simulate.rs` — 带真值的模拟器:结构化内容(标题、段落、列表、引用、代码、表格、分隔线、图片)、11 种编辑操作、按批次提交,逐块与真值比对。
- `tests/properties.rs` — 属性测试。结果见 ADR-012「Block identity spike results」。
