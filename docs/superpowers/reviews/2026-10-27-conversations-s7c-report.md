# S7c 开发报告：Task 0–5

日期：2026-10-27。分支：`feat/conversations-review`。工作目录：`/private/tmp/digger-s7c`。

按用户最新指示，**完成 Task 5 后停止，仅编写本报告**。Task 0–5 已实现并分别通过规格、代码质量审查；代码截止提交为 `e9c79489533450598f76b5717947619c22914ac7`。**Task 6 未开展，最终实现的 workspace 全量门禁尚未执行，GUI 目测验收未完成。** 本报告不代表全部 S7c 验收通过。

## 范围与边界

从 Digger `main 4495075` 建立独立临时 clone。当前权限不能写原仓库旁的 sibling worktree，采用交接允许的 `/private/tmp/digger-s7c`；原 Digger 主工作区的 13 份文档删除及 `.qoder/` 未由本任务改动，Dozer 工作区也未由本任务写入。

依赖保持 `libs-2026-10-26b`（Dozer `c76302fc`）。与基线相比，workspace/desktop 的 Cargo.toml、Cargo.lock、Info.plist 无改动。未修改共享库、review-trace 页面、守护进程、协议或版本号；未合并、推送、打 tag、发布或安装。删除项目的 transcript 清理仍未接入，总结能力仍关闭。

## 实现与提交

| Task | 提交 | 实现结果 | 默认 Rust 测试净增 |
| --- | --- | --- | ---: |
| 0 | `af5b407`、`88918df` | 抽取私有 `native_host`，Usage/Todo 复用单槽、epoch、规划、导航限制、缩放、离屏停放及焦点归还 | 11 |
| 1 | `13c6f8f` | ReviewNative、`digger-review://review-trace/`、原库资源应答、逐代数据槽、严格 URL/nonce 校验、产品消息与失败通知接线 | 20 |
| 2 | `b3e9c5e`、`662f7e7`、`a69aa4e` | 库审阅视图、正常取数与加载更多、缓存隔离、实际 shell 几何及真实控件不遮挡验证 | 18 |
| 3 | `85bf5ad`、`0d42f6b` | 覆盖层与窗口生命周期、真实最小化/尺寸查询、过期回复过滤、最小化期间正常尺寸保留 | 21 |
| 4 | `596cf11` | 七组自造 JSONL、十种画廊状态、安全种子与启动脚本、逐状态对照清单 | 2 |
| 5 | `b51dc87`、`1992693`、`e9c7948` | 真实 NSView 探针入口、快速切换及故障数据回归、全量分页、自检尺寸确认、nonce 回绕、回归记录 | 3 |
| 合计 | Task 0–5 | 代码及逐任务审查完成；人工结果待验证 | **75** |

审阅始终使用库内同一份页面和 `view::review_pane`。普通点行、分页均走库的服务/update；没有另写取数或页面协议。能力保持默认关闭，`LinkedTodos` 为空实现。

切项目先将旧实例的协议数据槽解绑为 `None`，使其 `data.json` 返回 404，再绑定新项目；保留原项目 WorkspaceState 的缓存供切回。Loaded 和文档错误同时核对 generation、完整页面 URL 与 `_r`，关闭旧 owner 后的事件不影响新 owner。

共享宿主把原生导航、缩放、摆位和脚本执行错误回传到现有通知条及 stderr。已经显示的审阅出错后会实际安排 Park，避免遮住 iced 错误文案；合法当前 Loaded 仍可显示页面自己的错误兜底。

## 测试数量与实际执行范围

开工在 `git archive` 干净快照（路径不含 digger，补入 git 历史、独立 target/HOME/数据根）执行基线：

- `cargo test --workspace --no-fail-fast`：**exit 101；712 passed / 1 failed / 0 ignored，总计 713**。
- 唯一失败：`a_restart_after_a_kill_reaps_the_recorded_orphan_and_leaves_unrelated_processes_alone`，预期 Killed，实际 Stale。
- 对编译后的该测试二进制精确隔离运行三次：**3/3 失败，均 exit 101**。当前沙盒连查询自身 PID 的 `/bin/ps` 都被拒绝，进程身份检查按保守路径处理。未改断言、未绕开限制。
- 证据：`/private/tmp/s7c-baseline.log`、`s7c-baseline-totals.json`、`s7c-baseline-recovery.json`、`s7c-ps-probe.json`。

最后实际完成的 desktop 测试为 **601 unit + 7 integration = 608 passed**。画廊另有 **4 个显式 example 测试**，Python 安全/隔离测试另有 **6 个**；它们不冒充默认 workspace 测试数。

预计默认 workspace 总数：`713 − 0 + (11 + 20 + 18 + 21 + 2 + 3) = 788`。其中 desktop unit 为 `528 + 73 = 601`，integration 为 `5 + 2 = 7`。**788 是清点后的预期总数，尚无最终全 workspace 通过结果。** 原占位相关测试随新行为更新断言，未减少测试数量。

原 `todo_native` 的 **7 个测试原文完全保留**；原 `usage_native.rs` 没有内联测试（0 → 0），其既有产品测试继续通过。核对记录：`/private/tmp/s7c-preserved-native-tests.json`。

| 实际命令 | 退出码与结果 | 证据 |
| --- | --- | --- |
| `cargo test -p digger-desktop -- --test-threads=1` | **0**；601 unit + 7 integration | `/private/tmp/s7c-task5-resize-wrap-final.log` |
| `cargo test -p digger-desktop --example conversations_gallery -- --test-threads=1` | **0**；4 example tests | 同上 |
| `cargo build -p digger-desktop --example conversations_gallery` | **0**；画廊及桌面库构建 | 同上 |
| `cargo clippy -p digger-desktop --all-targets -- -D warnings` | **0** | 同上 |
| `cargo fmt --all -- --check` | **0** | 同上 |
| `python3 -m unittest discover -s scripts/tests -p 'test_*conversations.py' -v` | **0**；6 tests | `/private/tmp/s7c-task5-final.log` |
| 实际 Dozer 几何/HOME 隔离探针，`cargo test -p dozer-app s7c_reference_probe::s7c_reference_numbers_and_home_isolation -- --nocapture` | **0**；只在临时源码快照运行 | `/private/tmp/s7c-dozer-geometry.log` |

Task 3 首次 desktop 运行有一个新增拖动测试使用了夹取区间内的坐标，导致无法产生 20 个不同位置；修正夹具范围后验证两种分隔线的 20 个不同位置。此项属于确定性夹具问题，未归类为偶发失败，也未弱化产品断言。测试最初缺 API 的编译 red、有效运行时 red 和夹具修正均在各任务记录中区分。

逐任务命令、red/green 及审查修复记录：`/private/tmp/s7c-task0-validation.md` 至 `/private/tmp/s7c-task5-validation.md`。

**尚未执行最终实现的以下门禁：** 干净快照 `cargo build/test/clippy --workspace`，最终依赖检查（host 含 self-test、shared、shell-regions）、Cargo.lock 重复检查。按用户停止要求未再启动这些工作；没有据现有定向结果宣称 workspace 全绿。现有 `block v0.1.6` future-incompatibility 提示未改动。

## Review Focus 对应证据

| 重点 | 已有测试/证据 | 范围限制 |
| --- | --- | --- |
| 加载与过期事件、nonce 回绕 | `loaded_matches_requires_the_current_nonce_and_rejects_garbage_urls`；`two_quick_clicks_show_only_the_second_rows_detail_even_if_the_first_loaded_event_arrives_last`；`load_more_appends_and_renavigates_with_a_new_nonce` | 最后一项实际点击 iced 按钮，经服务/库 update 验证普通 nonce 与 MAX→0，拒绝旧 MAX Loaded；真实 wry 点击穿透未验 |
| 可见性与关闭顺序 | `opening_the_native_agent_filter_menu_keeps_the_review_webview_visible`；`the_first_frame_after_becoming_visible_is_parked_not_shown_until_loaded`；`window_closing_destroys_it_before_shutdown`；窗口样本和最小化持久化测试 | 实际进入 update_host，检查规划及 iced runtime 动作；不冒充真实 NSView 操作结果 |
| 跨项目隔离 | `project_replacement_unlinks_old_protocol_closures_and_preserves_cached_snapshots`；`switching_back_shows_the_previous_projects_detail_again`；`closing_an_unrelated_project_preserves_the_live_owner` | 缓存与迟到事件有自动验证；第一帧是否闪旧内容仍待 GUI |
| 压力及恶意内容 | `twenty_distinct_rows_with_project_filter_resize_collapse_and_close_reject_stale_documents`；`huge_load_more_reaches_all_5000_rounds_without_duplicates`；`hostile_mixed_broken_and_bad_only_preserve_real_parser_semantics` | 数据、折叠、分页、序列化测试通过；不证明浏览器不执行脚本、无网络、不卡顿或无布局溢出 |
| Dozer 外观对照 | `review_bounds_matches_dozers_numbers_for_a_known_window`；真实 shell 几何、真实控件缩放/不遮挡测试；[逐状态清单](../notes/2026-10-18-conversations-parity.md) | 静态/无界面几何通过，逐状态截图和视觉差异裁决尚未完成 |
| 真实探针入口 | `actual_probe_is_scheduled_after_destroy_and_stale_results_are_rejected`；`resize_stage_emits_no_probe_until_actual_matching_window_acknowledgement` | 探针读取真实 NSView，但本环境未运行原生自检；测试证明调度、代数和确认门槛 |

## 几何与修订

真实 Dozer 函数在锁定源码快照中算出：1200×800、右栏默认分割时为 `(700, 84, 274.4, 690)`。Digger 实际 shell 路径为 `(701, 85, 275.2, 689)`，每项差值不超过 1 像素，下沿均为 774。

Dozer 使用 `status_bar_height=26`；Digger 共享 shell 使用 `footbar_height=22` 并扣 zone padding。审阅底部留白由导出度量计算，默认为 `26−22−1=3`，没有写死补偿偏移，也未改其他面板或共享 shell。原稿额外减 8px 的底边假设与真实 Dozer 函数不符，采用真实函数结果。

Digger 去除 Detail 头部重复 padding，补回同款分割线；真实控件在 0.5×、1×、2× 的标题、分割线、折叠按钮和长通知关闭按钮均有隔离进程布局/点击测试。Dozer 头部重复 padding 是否构成视觉差异仍待目测，未替用户接受。

## 夹具、画廊与安全隔离

七组夹具均为自造数据，已验证在 git archive 中完整且字节一致。复用真实 Claude parser、trace-detail 提取和库的折叠流程，未复制解析器。`huge` 为 5000 回合/10000 turns；hostile 含 3MB 以上单回合、10 万字符无空格行、HTML/script/javascript、零宽/RTL 等。

种子脚本拒绝缺参、真实 HOME、TMPDIR 根及之外、前缀伪装和符号链接逃逸；只写临时夹具，不导入、不连接守护进程、不调用模型。启动器显式覆盖 HOME 和 **DIGGER_DATA_DIR**、移除继承的 DOZER_DATA_DIR。预检发现的 DATA 误用已修复，并以恶意继承变量、execve 探针和实际 Rust config resolver 回归验证。

画廊有十种状态。加载状态仅在 example 内冻结真实 parsed entries/snapshot 的 pre-DocumentLoaded 阶段；ready 状态依赖真实 IPC。`agent-filter-open` 明确需要用户点击原生筛选控件，没有伪造已打开菜单或新增 iced 覆盖层。

## GUI 与性能：已验和未验

已尝试隔离画廊，启动 PID 67976，临时根为 `/private/var/folders/9q/8pyxcmxn3_l8r62v4w_gzsnh0000gn/T/s7c-conversations-fktcndb4`。日志出现 LSModifyNotification / hiservices XPC connection invalid；Orca 读取该 PID **exit 1 / app_not_found**，应用列表查询 **exit 0**、该 PID 无匹配。未获得窗口，**截图为无**，未操作用户正在运行的应用，也未代用户点击。

证据：`/private/tmp/s7c-gallery-observation/{launch.log,process.json,observation.json,app-registry.json}`。此结果不能证明产品渲染正确或错误；本报告保持 **GUI 未验**。

无界面 huge 数据测试单独测得 elapsed 9.716s、user 9.666s、system 0.044s、max RSS 39,567,360 bytes（37.7 MiB），Python 子进程测量 exit 0。`/usr/bin/time -l` 包装器因沙盒拒绝 sysctl 而 exit 1，未采用其缺失的 RSS。以上均不含 WKWebView 渲染，不能作为 UI 性能结论。

真实网络/CSP 违规、脚本执行、浏览器布局和 CPU/RSS、实际 kill diggerd、Dozer/Digger 同机并存及键盘/IME 均未验。没有修改页面或新增 IPC 来绕过观测限制。

## 已知限制与待验收

- 混合 `broken` 文件跳过坏行并保留 2 个有效 turns；全坏行/empty 则没有 entries。没有伪造“坏行必导致整体失败”。
- 库内 iced Load More 已验证存在及消息绑定；真实 native webview 是否盖住它尚未确认，按交接保留 Dozer 现状，不改页面。
- 创建原生子视图失败后，切另一行仍可能报告缺失子视图；隐藏再显示或切项目会重建。错误可见，未宣称自动重试已实现。
- 真实后端取数尚未返回时，库的空 entries 当前归为 Ready/无内容；未在 Digger 另造加载状态。
- 原生菜单、最小化恢复（Tick 后备查询最多约 1 秒）、全屏、跨显示器缩放、焦点归还及中文候选窗仍需真机验证。

可从本目录运行：

```sh
scripts/launch-conversations-fixture.sh gallery --state list-and-detail
scripts/launch-conversations-fixture.sh gallery --state list-and-detail --selftest-native
scripts/launch-conversations-fixture.sh digger
```

原生自检会检查真实 NSView 的五个阶段；尺寸检查必须收到同一窗口 1000×760 的实际确认，未确认会超时失败。**该自检尚未运行。** 真实 Digger 的历史导入仍需用户明确点击。

用户目测清单（全部待验）：

- 审阅位置、头部、空/加载/就绪/错误与 Dozer 对齐；折叠/展开、拖两类分隔线、缩放/最小化/全屏。
- 打开原生 Agent 筛选时审阅保持可见；iced 确认框不被遮挡。
- 切项目及加载中关闭页签不串内容、不闪旧页；关闭后探针句柄数为 0。
- 从详情切回 Agent 能打字；搜索字符不进终端；中文拼音候选位置、选择、提交、删除和取消正常。
- huge 加载全部页、hostile 的执行/网络/CSP/溢出表现、mixed-broken 与 empty 的实际后端表现。
- 只停止隔离的 diggerd，验证缓存详情、不可用原因、错误和重试；Dozer 与 Digger 数据互不串用。

详见[对照清单](../notes/2026-10-18-conversations-parity.md)与[回归记录](../notes/2026-10-18-conversations-regression.md)。

## 停止点与后续裁决

按用户要求停在 Task 5；Task 6 的设计状态、窗口骨架文档、ADR 同步及独立 open-items 文档未开展，最终全量门禁未启动。

后续仍需用户裁决：删除项目只清数据库副本还是也删 HOME 中 transcript 原文件（现有服务可能两者都删）；总结生成开通与费用；页签部件、菜单弹层及 review/native_host 样板上移 byteui/bytehost 的跨仓库工作。本期没有自行处理这些事项。
