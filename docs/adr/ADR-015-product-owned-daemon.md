# ADR-015: Product-Owned Daemon Architecture

## Status

Accepted (2026-10-23, S6a implementation complete)

## Context

Digger and Dozer are two products that share significant infrastructure: daemon process, hook binary, client library, project store, and SQLite database. Prior to S6a, Digger had no daemon of its own and relied on Dozer's infrastructure. This created coupling issues:

1. **Product identity**: Both products need separate data directories, sockets, databases, and session environment variables to coexist on the same machine.
2. **Independent evolution**: Digger needs to evolve its daemon behavior (e.g., which features are enabled) independently from Dozer.
3. **Hook coexistence**: Each product's hook binary must only manage its own hook entries in `~/.claude/settings.json`, never touching the other product's entries.
4. **Database ownership**: Digger needs its own `digger.db` file, separate from Dozer's `dozer.db`, with migration from the legacy `projects.db`.

## Decision

Adopt a "product-owned daemon" architecture where each product (Digger, Dozer) owns its daemon binary, hook binary, database file, and data directories, while sharing the underlying implementation as library crates.

### Implementation Structure

**Shared library crates** (in `dozer-mcp-product`):
- `dozer-core`: Product identity abstraction (`ProductId`), protocol definitions
- `dozerd`: Daemon implementation (configurable via `DaemonConfig` + `DaemonFeatures`)
- `dozer-hook`: Hook binary implementation (parameterized by `ProductId`)
- `dozer-client`: Client library for connecting to daemon
- `dozer-store-project`: Project storage layer (SQLite)

**Product-specific crates** (in each product's repository):
- **Digger** (`digger-s6a`):
  - `digger-paths`: Product identity (`ProductId::custom("digger", "DIGGER_SESSION_ID", "digger-hook", "diggerd.sock", "digger.db", ...)`)
  - `diggerd`: Thin binary entry point (3 lines: construct `DaemonConfig`, call `dozerd::daemon::run`)
  - `digger-hook`: Thin binary entry point (1 line: call `dozer_hook::run_main(digger_paths::product())`)
  - `digger-desktop`: Main app, responsible for launching `diggerd` during boot

- **Dozer** (`dozer-mcp-product`):
  - `dozer-core::paths::dozer()`: Product identity (`ProductId { name: "dozer", session_env: "DOZER_SESSION_ID", hook_bin: "dozer-hook", socket_file: "dozerd.sock", db_file: "dozer.db", ... }`)
  - `dozerd/src/main.rs`: Thin binary entry point
  - `dozer-hook/src/main.rs`: Thin binary entry point

### Product Identity Fields

Each `ProductId` specifies:
- `name`: Product name ("digger" or "dozer")
- `session_env`: Session environment variable name ("DIGGER_SESSION_ID" or "DOZER_SESSION_ID")
- `hook_bin`: Hook binary name ("digger-hook" or "dozer-hook")
- `socket_file`: Daemon socket file name ("diggerd.sock" or "dozerd.sock")
- `db_file`: Database file name ("digger.db" or "dozer.db")
- `dirs`: Data directory structure (Fixed for Digger, Project for Dozer)

### Feature Flags

`DaemonFeatures` controls which daemon capabilities are enabled:
- **Digger**: `DaemonFeatures::minimal()` (all features off: no app-host, no transcript backfill, no IDE bridge, no summary scheduler)
- **Dozer**: All features enabled (matches pre-S6a behavior)

### Database Migration

Digger's `digger.db` is created by migrating from the legacy `projects.db`:
1. `ProjectHost::open_in(db_path, legacy_db)` checks if migration is needed
2. If legacy exists and target is empty: ATTACH legacy, copy `projects`/`memories`/`memory_history` tables in a transaction, rename legacy to `projects.db.migrated`
3. Set SQLite WAL mode on `digger.db` for concurrent access (Digger-specific, not set by Dozer)

### Hook Coexistence

Each product's hook binary only manages its own entries:
- `digger-hook install claude` writes entries with "digger" in the command
- `dozer-hook install claude` writes entries with "dozer" in the command
- Both can coexist in `~/.claude/settings.json` without conflict

## Consequences

### Positive

1. **Independent evolution**: Digger and Dozer can enable/disable daemon features independently
2. **Clean coexistence**: Two products can run on the same machine with isolated data
3. **Code reuse**: Shared infrastructure remains in library crates, avoiding duplication
4. **Clear ownership**: Each product's data (database, socket, logs) is clearly separated
5. **Migration path**: Legacy `projects.db` is automatically migrated to `digger.db` with WAL mode

### Negative

1. **Build complexity**: Each product must build its own daemon and hook binaries
2. **Patch dependencies**: Digger uses local `[patch]` to override dozer crates during development (not committed to git)
3. **Testing overhead**: Coexistence regression tests require isolated data directories for both products

### Neutral

1. **Binary size**: Digger now ships three binaries (digger, diggerd, digger-hook) instead of one
2. **Runtime behavior**: Digger's daemon runs with minimal features; Dozer's daemon runs with all features

## MCP

`digger-mcp` 是 Digger 自己的 MCP 服务,和 `digger-hook` 一样随 `Digger.app/Contents/MacOS/` 交付。它暴露:

- 记忆:`write_memory` / `list_memories` / `get_memory`,读写的是 `diggerd` 里的项目共享记忆(`digger.db`),与 Project 面板同一份;
- 预览桥接:`get_preview_context` / `preview_navigate` / `locate_in_file`,以及精确编辑 `apply_precise_edit`。这些工具经 `diggerd` 的中转落到 GUI 的预览面板:Digger 在 `boot` 后把当前项目 Files 面板的活动页签上下文推给 `diggerd`(切页签/关页签/切换项目立即,其余 250ms 防抖),并轮询 Agent 下行命令;目标项目不在前台时立即回 `NotFound`,不会挂起。它们与 Dozer 的同名工具共用同一份 `dozer-mcp` / `dozer-panel-preview` 代码。

todo 与总结工具在 Digger 侧仍关闭。不会出现"调用后无可见效果"的工具。

- 与 `dozer-mcp` 并存:两者的配置条目分别是 `digger` / `dozer`,安装与卸载互不覆盖;Dozer 会话里启动 `digger-mcp`、Digger 会话里启动 `dozer-mcp` 时都进入**零工具安静模式**(握手成功、工具为空、不写 stderr),Agent 的 MCP 状态里不是失败。
- 自动注册的克制(D7):每个 Digger 进程内每种 Agent 只执行一次 `digger-mcp install`;同名手写条目被保护(退出码 3)时不覆盖、只提示一次;缺二进制 / 超时 / 非零退出都不阻塞建会话。
- 回合结束且 Project 面板可见时刷新记忆列表(只调库的 `on_activate`,不补 README / git / 磁盘占用)——这是相对 Dozer 的有意改进。

## Agent 面板

Agent 面板与会话也走这条产品自有守护进程:面板的会话服务接缝(`dozer_services::SessionService`)由 `Daemon::sessions()` 包装同一个 `dozer_client::Client`,连的是 `diggerd` 的 socket;hook 安装器 spawn 同目录的 `digger-hook install <agent>`,不碰 Dozer 的任何配置。

- 关项目页签 = kill 该项目全部会话;关窗口只 detach(会话随 `diggerd` 存活);退出前等在途 kill(3 秒上限)。
- `digger-hook` 与 `dozer-hook` 各自写自己产品的配置条目,同机并存不互相覆盖。
- 页签部件与菜单弹层原先是 Digger 侧的临时复制(`agent_tabs.rs`、`menu_overlay.rs`、`tab_chrome.rs`);**2026-10-28 已迁入 byteui 0.5.0**(`interaction::panel_tabs`、`interaction::menu`、`feedback::tooltip`、`theme::region::context_menu`),Dozer 与 Digger 共用,这三个文件缩成库显示参数到 byteui 部件的接线。可见变化:菜单背景统一不透明;预览页签提示气泡改为 byteui 统一样式;Agent 溢出下拉的宽高上限不再随 UI 缩放(与 Dozer 一致,scale=1 时不变)。可信原生子 webview 执行器(原 `native_host.rs`)**同日迁入 bytehost v0.4.14 的 `bytehost_desktop::native_child`**(X1 阶段 B),usage/todo/group_chat/review 四个原生面板改为消费;`markdown/host.rs` 评估后不并入(HTML 载入 + IPC + `about:blank` 导航 + 剪贴板开,模型与设置都不同)。

## 会话面板

会话面板(`digger.conversations`)读写的是各自产品自己的库:Digger 经 `diggerd` 读写 `digger.db`,Dozer 经 `dozerd` 读写 Dozer 的库,两者互不串用。历史导入在 Digger 里由用户显式触发,不静默复制用户的历史。

## 群聊面板

群聊面板(`digger.group_chat`)读写各自产品的库:Digger 经 `diggerd` 读写 `digger.db`。成员 Agent 的发言由各自产品的守护进程以只读方式拉起(Claude 工具白/黑名单、Codex `--sandbox read-only`),Digger 不增加也不放宽这一点;建群、选群、增删成员、删群本身不拉起任何 Agent 进程。

## Related ADRs

- **ADR-014**: Agent Panel S6 Design (references this ADR for daemon architecture)

## Implementation References

- S6a implementation plan: `/docs/superpowers/plans/2026-10-18-agent-s6a-daemon-identity.md`
- S6a implementation report: `/docs/superpowers/reviews/2026-10-23-s6a-partc-report.md`
- Product identity code: `crates/paths/src/lib.rs` (Digger), `dozer-core/src/paths.rs` (Dozer)
- Daemon entry point: `crates/diggerd/src/main.rs`
- Hook entry point: `crates/digger-hook/src/main.rs`
- Database migration: `crates/store/src/migrate.rs`
