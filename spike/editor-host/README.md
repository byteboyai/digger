# editor-host spike

一次性验证 ADR-001 的编辑器宿主边界(验证项 1、2 的一部分)。随时可删。

## 运行

```bash
cd host && npm install && npm run build      # 产物 host/dist-tiptap/ 与 dist-milkdown/,由 Rust include_str! 嵌入
cd .. && cargo test                           # 桥接校验 + AST 投影/diff 单测
cargo run                                     # 手工:窗口左侧是 iced 面板,右侧是 Tiptap 宿主
SELFTEST=basic cargo run                      # 自动:编辑往返、块 ID 稳定、视口随窗口缩放
SELFTEST=merge cargo run                      # 自动:基线过期 → 自动合并 / 冲突 / 采用哪一方 / 干净宿主接收官方变更
SELFTEST=fuzz cargo run                       # 自动:随机插入/删除/移动/改类型的下行增量,逐步比对宿主与正式版本(FUZZ_SEED、FUZZ_STEPS、BLOCKS 可调)
SELFTEST=inputrule cargo run                  # 自动:Markdown 输入规则 "## " 后块 ID 是否保留
BLOCKS=2000 SELFTEST=perf cargo run --release # 大章节:加载/按键事务/帧大小/Rust 解析与 diff
HOST=milkdown cargo run                       # 以上任一命令加 HOST=milkdown 即换用 Milkdown 宿主
./run_recovery_test.sh                        # 自动:崩溃(abort)后重启,恢复条的接受/丢弃/基线落后三种路径
```

(本机 cargo 配置了 `build.target`,二进制在 `target/aarch64-apple-darwin/debug/`。)

## 结构

- `src/bridge.rs` — `HostEnvelope`、入站校验(绑定、封闭白名单、大小上限、seq 严格递增)。
- `src/merge.rs` — 块级三方合并(base/local/official),冲突返回 id 列表,可按 Mine/Official 解决。
- `src/store.rs` — 正式状态 + 历史 + 恢复日志的 JSON 持久化(原子 rename + fsync;SQLite 的替身)。
- `src/doc.rs` — `Delta`(块级增量:changed / removed / placements)的解析与 `apply_delta`、线性 `diff`;
   极简 AST(段落/标题)、AST→ProseMirror 投影、候选文档解析(拒缺失/重复 blockId、不支持节点)、语义操作 diff。
- `src/main.rs` — iced 窗口;`window::run` 里 `build_as_child` 建 wry 子视图;`evaluate_script` 下行,`ipc_handler` 经 channel 上行;`resize_events` 同步子视图 bounds。
- `host/src/core.ts` — 与编辑器库无关的宿主核心(桥接、防抖/单飞行、rebase/conflict、自测命令、blockId 与变更插件);`entry-tiptap.ts` / `entry-milkdown.ts` 只建 ProseMirror 视图。原 Tiptap 宿主的特性:`blockId` 全局属性 + `appendTransaction` 补/去重 id;防抖编辑 + 单飞行 + 提交确认;`rebase`/`conflict`/`official_advanced` 处理(有未发送编辑时不覆盖文档);防抖快照;JS 错误经 `failed` 回传。

## 结果

见 ADR-001「Spike 结果」一节。
