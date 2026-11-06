# digger

## Architecture documents

- [Research report](docs/ByteBoy_Digger_v1_%E8%B0%83%E7%A0%94%E5%88%86%E6%9E%90%E6%8A%A5%E5%91%8A.md)
- [Domain Model v0.2](docs/ByteBoy_Digger_Domain_Model_v0.2.md)
- [Logical Domain Model v0.3](docs/ByteBoy_Digger_Logical_Domain_Model_v0.3.md)
- [Runtime Architecture v0.1](docs/ByteBoy_Digger_Runtime_Architecture_v0.1.md)
- [Technical Architecture v0.1](docs/ByteBoy_Digger_Technical_Architecture_v0.1.md)
- [ADR-001: iced + wry Desktop Shell and Editor Host Boundary](docs/adr/ADR-001-iced-wry-desktop-shell.md)
- [ADR-003: Project SQLite Consistency Island and Transaction API](docs/adr/ADR-003-project-sqlite-consistency-island.md)
- [ADR-006: Writing Document Representation](docs/adr/ADR-006-writing-document-representation.md)
- [ADR-011: Embedding bytehost as the Application Host](docs/adr/ADR-011-bytehost-app-host-embedding.md)
- [ADR-012: Markdown Text as the Authoritative Chapter Content](docs/adr/ADR-012-markdown-text-as-authoritative-content.md)
- [ADR-013: A First-Party Markdown Panel from the md-engine Spike](docs/adr/ADR-013-markdown-panel-from-md-engine-spike.md)
- [ADR-016: Browser Panel and the Untrusted-Content Webview](docs/adr/ADR-016-browser-panel-untrusted-content.md)(视图画廊:`cargo run -p digger-desktop --example browser_gallery -- --list`)
- [S1 窗口骨架:共享 bytehost-chrome](docs/superpowers/specs/2026-10-14-window-shell-design.md)
- [S2 用量面板接入"列表 | 内容"配对](docs/superpowers/specs/2026-10-15-usage-pair-design.md)
- [共享设置：Dozer 抽取到 bytehost，Digger 接入](docs/superpowers/specs/2026-10-29-shared-settings-design.md)（[实施计划](docs/superpowers/plans/2026-10-29-shared-settings.md)）
- [统一设置模块与对话框宿主](docs/superpowers/specs/2026-11-03-unified-settings-dialog-design.md)（[实施计划](docs/superpowers/plans/2026-11-03-unified-settings-dialog.md)）
- [统一设置共享版本发布记录](docs/superpowers/reviews/2026-11-04-shared-settings-release.md)：bytehost `v0.4.23` / Dozer `libs-2026-11-04a`；无 patch 发行验证与实机边界。
- [面板开发前最小稳定化收口 Spec](docs/superpowers/specs/2026-11-05-panel-development-readiness-design.md)（[实施计划](docs/superpowers/plans/2026-11-05-panel-development-readiness.md)，[验收报告](docs/superpowers/reviews/2026-11-05-panel-development-readiness-acceptance.md)）：用量异步隔离、最新代码基线验证与双产品最小实机冒烟。
- [写字台真实文稿与后续写作能力 Spec](docs/superpowers/specs/2026-11-05-writing-workbench-completion-design.md)（[实施计划](docs/superpowers/plans/2026-11-05-writing-workbench-completion.md)，[基线](docs/superpowers/notes/2026-11-05-writing-workbench-baseline.md)）：W1 文稿持久化、恢复草稿、镜像与项目入口。
- [写字台备份与恢复操作](docs/superpowers/notes/2026-11-06-writing-backup-recovery.md)：一致数据库快照、Markdown 导出、外部文件和恢复草稿保留。
- [代码恢复性、性能与工程质量优化](docs/superpowers/specs/2026-11-02-code-reliability-and-performance-design.md)（[实施计划](docs/superpowers/plans/2026-11-02-code-reliability-and-performance.md)）
- [会话面板 scale 用例 fork 死锁修复](docs/superpowers/specs/2026-11-04-conversations-scale-test-fork-hang-design.md)（[实施计划](docs/superpowers/plans/2026-11-04-conversations-scale-test-fork-hang.md)，[验收报告](docs/superpowers/reviews/2026-11-04-scale-test-fork-acceptance.md)）
- Engine spike: `spike/md-engine` (CodeMirror 6 vs ProseMirror under ADR-012)
- Block identity spike: `spike/block-identity` (stable block ids across edits, with property tests)
- Parser agreement spike: `spike/parser-agreement` (Rust vs Lezer block boundaries, with a conformance check)
- [Markdown editor initial requirements (Dozer and Digger)](docs/byteboy_markdown_editor_initial_requirements.md)
- [GitHub projects code feature analysis v0.1](docs/ByteBoy_Digger_GitHub_Projects_Code_Feature_Analysis_v0.1.md)

## 界面语言

设置面板的「语言 / Language」页提供「跟随系统 / English / 简体中文」，选择后立即应用到设置和已接入多语言的共享面板，并保存到 Digger 配置目录的 `language.json`，重启后恢复。跟随系统使用 macOS 首选语言；不支持的语言回退英文。部分 Digger 自有文案仍为中文。

## 共享库依赖守卫

`scripts/check-shared-deps.sh` 检查 bytehost / byteui / bytegit / dozer 等 `byteboyai/*` 共享库的依赖:拦下本地路径 `[patch]`、指向本地路径的 `Cargo.lock`、远端不存在的 tag/rev、同一共享库的多份版本,并提示落后。规则与设计见 `docs/superpowers/specs/2026-10-16-shared-deps-guard-design.md`。

- 启用 pre-push 钩子(每个克隆一次):`git config core.hooksPath .githooks`。推 `main` 或 tag 时阻断,推其他分支只警告;确认要绕过时用 `git push --no-verify`。
- 随时手动跑:`scripts/check-shared-deps.sh`(检查工作区)或 `scripts/check-shared-deps.sh --rev HEAD`(检查已提交内容)。
- 本地联调共享库:复制 `scripts/dev-patch.example.toml` 到 `.cargo/config.toml`(已被忽略,不入库)。
- 脚本正本在 bytehost 的 `tools/shared-deps-guard/`,本仓库是 vendor 副本(见 `scripts/check_shared_deps.py` 里的 `SOURCE_TAG`)。


## Code reliability checks

See [the implementation checklist](docs/superpowers/plans/2026-11-02-code-reliability-and-performance.md) and [latest verification with raw samples](docs/superpowers/reviews/2026-11-03-code-optimization-completion.md).

PR CI runs editor unit tests (excluding the book stress test), representative properties, store/app-host/paths tests, web checks and embedded bundle consistency. The scheduled/manual slow workflow retains the full Release property suite and book stress test, compares production Bridge timings with the pre-optimization revision, and measures SQLite at 1k/10k/100k records.

```bash
cargo test -p digger-editor --lib -- --skip typing_in_a_book_parses_a_few_blocks_not_the_book
cargo test --release -p digger-editor
for scale in 0.5 1 2; do
  BYTEUI_ICON_SCALE="$scale" cargo run -p digger-desktop --bin icon_scale_widget_probe
done
(cd crates/desktop/web && npm ci && npm run check && npm test)
bash scripts/build-editor-web.sh --check
CARGO_TARGET_DIR="$PWD/target" bash scripts/bench-editor-sync.sh /private/tmp/digger-performance
cargo run --release -p digger-store --example usage_bench
```

The GUI harness includes separate production IPC/paint samples (T25). Run `bash scripts/run-editor-selftest.sh` in a normal macOS graphical session; set `DIGGER_EDITOR_FAIL_FIRST_BUILD=1` to verify the visible Retry button. Process recovery tests require process-query permissions. Release checks must use the declared dependencies without personal path patches; the current shared-version blockers are recorded in the latest verification.
