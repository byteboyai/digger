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
- Engine spike: `spike/md-engine` (CodeMirror 6 vs ProseMirror under ADR-012)
- Block identity spike: `spike/block-identity` (stable block ids across edits, with property tests)
- Parser agreement spike: `spike/parser-agreement` (Rust vs Lezer block boundaries, with a conformance check)
- [Markdown editor initial requirements (Dozer and Digger)](docs/byteboy_markdown_editor_initial_requirements.md)
- [GitHub projects code feature analysis v0.1](docs/ByteBoy_Digger_GitHub_Projects_Code_Feature_Analysis_v0.1.md)

## 共享库依赖守卫

`scripts/check-shared-deps.sh` 检查 bytehost / byteui / bytegit / dozer 等 `byteboyai/*` 共享库的依赖:拦下本地路径 `[patch]`、指向本地路径的 `Cargo.lock`、远端不存在的 tag/rev、同一共享库的多份版本,并提示落后。规则与设计见 `docs/superpowers/specs/2026-10-16-shared-deps-guard-design.md`。

- 启用 pre-push 钩子(每个克隆一次):`git config core.hooksPath .githooks`。推 `main` 或 tag 时阻断,推其他分支只警告;确认要绕过时用 `git push --no-verify`。
- 随时手动跑:`scripts/check-shared-deps.sh`(检查工作区)或 `scripts/check-shared-deps.sh --rev HEAD`(检查已提交内容)。
- 本地联调共享库:复制 `scripts/dev-patch.example.toml` 到 `.cargo/config.toml`(已被忽略,不入库)。
- 脚本正本在 bytehost 的 `tools/shared-deps-guard/`,本仓库是 vendor 副本(见 `scripts/check_shared_deps.py` 里的 `SOURCE_TAG`)。
