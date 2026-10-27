# S8c 开发报告:群聊聊天页 webview(Task 2–6)

> **状态更新(2026-10-27):** 已合并进 `main`(`5583fe4`),并已发版 0.1.7;用户已完成人工 GUI 验收。下文「GUI 全部未验 / 停在分支上」为写报告时的状态。「待裁决」6 项仍未裁决,见 `../notes/2026-10-18-group-chat-open-items.md`。

日期:2026-10-27。分支 `feat/group-chat-chat`,工作目录 `../digger-s8c`,基线 Digger `main` `3e0fefc`(已含 S7c、S8b)。**停在分支上:没有合并、推送、打 tag、发版、安装、升版本号(原 Task 7 不在本任务内)。GUI 全部未验。**

## 范围与边界

- 依赖仍为 `libs-2026-10-26b`(Dozer `c76302fc`);`Cargo.toml`/`Cargo.lock` 无改动。**库、页面、`dozerd`、协议均未改。**
- Task 1(抽 `native_host`)已在 S7c 完成,本期直接复用;`native_host.rs`、`review_native.rs` 与基线逐字相同(`git diff main` 为空)。
- 主工作区的未提交改动(13 份已删除的 S6 文档、`.qoder/`)未触碰;未碰 `feat/preview-pairing` / `digger-p4`;未碰 `/private/tmp/digger-sandbox`、`/Applications` 里的应用。

## 提交

| 提交 | 内容 |
| --- | --- |
| `8784509` | Task 2–3:`group_chat_native.rs`、`group_chat_geometry.rs`、面板接线、覆盖层/可见性/生命周期测试 |
| `f9d4669` | Task 4:假 `claude`/`codex`、`with-fake-agents.sh`、`check-fake-agent-log.sh`、`launch-group-chat-fixture.sh`、Python 自测 |
| (启动器修正) | 启动器保留 GUI 环境,只去掉继承的 `DOZER_DATA_DIR` |
| `589ac6b` | 关窗等待聊天页销毁完成的测试 |
| (文档提交) | Task 6:spec 结果、ADR-015、对照/回归/遗留笔记、本报告 |

## 与原稿/交接的不同处

- **库 API 按真实代码:** 无 `caps` 参数;页面 URL 是 `?theme=dark`;`take_content_script` 返回的脚本已包好,宿主不再包装;转待办默认开启,`ShowTodoPanel` 映射为 `ShellMsg::ShowPanel(todo_id)`,待办面板可见时每秒轮询所以新条目会自己出现(未改待办面板)。
- **协议名** `digger-groupchat`,主机名 `group-chat-content`;导航白名单复用 `native_host::allows_navigation`;新窗口、下载一律拒绝(共享宿主)。
- **覆盖层:** iced 内模态使页面**停放并隐藏、不销毁**(而不是销毁重建),关闭后恢复;原生 Agent 筛选菜单不算覆盖层。
- **几何:** 公式 `chat_bounds` 与 Dozer 的 `chrome_top = 0` 分支同构:顶部内缩 = 区圆角余量,底部内缩 = 区圆角余量 + (状态栏 − 脚注)。Dozer 数字来自锁定快照里真实函数的探针,不是原稿猜的。
- **画廊状态未做。** 原稿 Task 4 的 11 个画廊状态假设聊天页能脱离守护进程渲染,但聊天内容是库页面、状态来自守护进程数据。改用真实应用 + 假 Agent 的指令(`FAKE:slow`/`fail`/`hostile`/`long`)摆这些状态,摆法写在对照清单。S8b 的 `group_chat_gallery` 保留不动(只覆盖列表)。
- **删除 `no_todo_text_anywhere_in_the_view`**(原本没有任何断言,且聊天页现在有转待办入口)。以 `the_failure_shell_names_the_reason_has_no_todo_text_and_retry_clears_it` 取代,条数不变。
- **列表收起的按钮条**(Dozer 没有)是已知偏离,见遗留。

## 测试数与执行范围

干净快照(`git archive` + 补 git 历史,路径不含 digger,独立 `CARGO_TARGET_DIR`):

| | 命令 | 结果 |
| --- | --- | --- |
| 基线 `3e0fefc` | `cargo test --workspace --no-fail-fast` | 807 通过 / 1 失败 = **808**;失败是已知偶发的 `apps::tests::closing_during_startup_waits_and_releases_the_root`,**单独重跑 3/3 通过**(该测试在基线并行跑时失败一次,本分支并行跑时通过) |
| 终态 `589ac6b` | 同上 | **844 通过 / 0 失败 / 0 ignored = 844** |

核对算式:`808 − 0 + 36 = 844`。新增 36 = `group_chat_native` 14 + `group_chat_geometry` 4 + `product::tests::chat_lifecycle` 18;`group_chat.rs` 内一条旧测试被替换(11 → 11)。既有测试条数未减:`native_host_tests` 11→11、`review_native_tests` 22→22、`todo_native` 7→7、`usage_native` 0→0、`conversations_tests` 67→67、`product_review_tests` 22→22。另有 Python 自测 10 个(不计入 workspace 总数)。

| 门禁 | 命令 | 退出码 |
| --- | --- | --- |
| 构建 | `cargo build --workspace` | 0 |
| clippy | `cargo clippy --workspace --all-targets -- -D warnings` | 0 |
| fmt | `cargo fmt --all -- --check` | 0 |
| host 依赖 | `bash scripts/check-host-deps.sh` | 0 |
| shared 依赖 | `bash scripts/check-shared-deps.sh` | 0(0 阻断 0 警告) |
| shell 区域 | `bash scripts/check-shell-regions.sh` | 快照里 2(找不到同级 `../dozer`);**在工作区里跑为 0**(对用户本机 `../byteui`、`../dozer` 的工作副本) |
| 假 Agent 自测 | `python3 -m unittest discover -s scripts/tests -p test_fake_agents.py` | 0(10 个) |

第一次门禁运行因磁盘写满失败(`No space left on device`,不是代码问题);清理了我自己在草稿目录里留下的旧构建产物后整套重跑。

**没做的:** `check-host-deps.sh --self-test`、Cargo.lock 重复检查未单独执行(`check-shared-deps.sh` 通过)。

## 抓到并修掉的缺陷

- 切项目时新建的聊天页沿用旧页的就绪状态,直接 `Show` 而不是先停放。由 `switching_the_project_replaces_the_page_and_a_new_page_is_not_ready` 发现;`plan` 现在只在同项目时继承就绪。我自己写的一条单测当时把这个错误行为固化成了断言,已改正。
- 加载超时是在取脚本时判定的,原先要晚一次同步才销毁页面;现在在同一次同步里重新计算目标。

## Review Focus 对应证据

| 重点 | 测试 | 范围限制 |
| --- | --- | --- |
| 就绪握手与推送 | `the_first_frame_is_parked_and_only_a_ready_page_is_shown`;`scripts_are_only_injected_into_a_built_current_child_of_the_same_project`;`a_ready_timeout_shows_the_failure_shell_…`(含失败后迟到 ready 不撤销失败、重试后重建) | 推送内容本身是库的;实际 `evaluate_script` 到达页面未验 |
| 流式与竞态 | 轮询订阅条件沿用 S8b;迟到事件按代数丢弃 | 真实流式、`diggerd` 中途死亡**未验** |
| 键盘与焦点 | 弹窗打开时终端按键被拦截(S8b);销毁/停放由共享宿主归还焦点(S7c) | 中文输入法、粘贴、⌘ 组合键、焦点归还**需用户** |
| 不可信内容 | `ipc_from_another_origin_is_dropped_…`、`oversized_or_malformed_ipc_…`、`forged_events_have_no_effect_and_never_spawn_an_agent`、协议 404 与遍历用例 | 页面不执行脚本、无网络请求、布局**需用户看** |
| 覆盖层与几何 | `chat_lifecycle` 系列、`the_real_default_shell_matches_dozers_chat_geometry`、分隔线 20 个位置 | 真实窗口里的遮挡关系**需用户** |

## 只读的证据

经真实 `dozerd::HeadlessGroupRunner` 对假 Agent 往返(在临时 Dozer 快照 `/private/tmp/s7c-dozer-reference` 里加的探针 `s8c_fake_roundtrip`,**未提交到 Dozer**):6 次调用(Claude/Codex 各含普通、长文、恶意、失败)全部成功/按预期失败,回复未被截断;记录下来的命令行经 `check-fake-agent-log.sh` 通过:Claude 带 `--allowedTools Read,Grep,Glob` 与含 Bash/Write 的 `--disallowedTools`,Codex 带 `--sandbox read-only`,无 `--dangerously-skip-permissions`/`-y`。这证明 `dozerd` 拉起成员的参数是只读的;**它不是 Digger 里端到端的验证**——Digger 的端到端需要用户运行假 Agent 环境后再跑检查器。

## 没验的(明说)

全部 GUI:窗口里的位置与遮挡、折叠按钮条、真实 WKWebView 加载与就绪握手、页面脚本到达、流式与取消的观感、恶意内容不执行/无网络请求、中文输入法、焦点归还、最小化/全屏/跨显示器、`diggerd` 中途死亡、2 000 条历史与 30 次连发的压力、同机并存、真实 Agent 抽检。清单见 `docs/superpowers/notes/2026-10-18-group-chat-parity.md` 与 `…-regression.md`。

## 给用户的验收命令

```sh
cargo build -p digger-desktop          # 用户自己的 target
scripts/launch-group-chat-fixture.sh prepare   # 看隔离根与命令
scripts/launch-group-chat-fixture.sh digger    # 隔离 HOME/数据根 + 假 Agent
scripts/check-fake-agent-log.sh <打印出的 fake-agents.log>   # 跑完后
```

## 待裁决(详见 `notes/2026-10-18-group-chat-open-items.md`)

创建失败时草稿丢失(要改库);列表收起的按钮条偏离 Dozer;`list_collapse_button` 库适配占位;跨仓库迁移;不能发言的三个成员种类是否隐藏。
