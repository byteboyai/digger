# S6d-2 Task 5 实施报告(键盘、指针焦点、粘贴复制、拖入、输入法)

日期:2026-10-23。分支 `feat/agent-sessions`(worktree `../digger-s6d2`),基线 `438c42e`(1b 复核通过后的顶端),追加提交,未改写历史。计划:Dozer 仓库 `docs/superpowers/plans/2026-10-18-agent-s6d2-sessions.md` Task 5;前置:输入法探针五项已由用户真机验证通过(2026-10-23)。

## 提交

- `5a9e346` feat(agent): S6d-2 Task 5 键盘、指针焦点、粘贴复制、拖入与输入法

库未改动,无新依赖。未合并 `main`、未打 tag、未推送、未发布。

## 实现要点

- **按键字节:** `agent_keys::event_bytes(&keyboard::Event, app_cursor)` 只处理 `KeyPressed`;`Key::Character` 用 `modified_key`(Shift+a → `A`、Shift+1 → `!`),具名键用 `key`;⌘ 组合键在 `key_bytes` 里恒为 `None`。
- **事件订阅:** `listen_with` 是无捕获函数指针,`agent_key_message` 对 `Status::Captured` 返回 `None`(被输入框等控件消费的按键不进终端)。鼠标按下取 `view` 每帧写入 thread-local 的最近光标位置;`window::Event::FileDropped` 转 `Msg::AgentDrop`。Agent 自己的选择器 / 关页签确认打开时键盘整体不投(thread-local `AGENT_KEYS_BLOCKED`)。
- **`AgentPanel::on_key`:** 窗口关闭中 / 面板不可见 / 无焦点 / 当前项目没有状态 / 没有活动会话一律丢弃。⌘C 只在有选区时 `clipboard::write`;⌘V `clipboard::read()` → `Msg::AgentPaste(project_id, text)`;其余经 `event_bytes` → 库 `Message::Input`。粘贴包装由库按终端当前 bracketed paste 模式完成。
- **指针焦点:** `on_pointer` 用 `terminal_rect.contains(point)` 置 `focus`;失焦清 `preedit`。`terminal_rect` 由 `update_host` 每轮几何同步写入(终端块矩形,窗口坐标)。
- **拖入:** 面板可见时把 `dozer_panel_term::input::dropped_path_bytes(path)` 写进活动会话(shell 转义 + 末尾空格)。
- **输入法:** 焦点在终端时用 `ImeSink` 包住终端块;提交走库的 `input::ime_commit`,预编辑存 `panel.preedit`(`PanelDisplay::ime_preedit` 读取),失焦 / 面板切走 / 项目切换都清空;迟到的旧项目预编辑丢弃。候选框矩形 = 终端块原点 + 内边距 + 页签条高度 + 光标格 × (单元格宽, 行高)。
- **双重输入风险(风险 1):** `decide_key` 在 `preedit.is_some()` 时返回 `None`,抑制组字期间的 `KeyPressed`。这是有意的保险;探针已验证候选框与提交正常,但"组字期间 iced 是否仍送按键"需要真机最终确认(见"待用户")。

## 测试(Task 5 计划里的 10 条,名字未改)

| # | 测试名 | 断言 |
| --- | --- | --- |
| 1 | `shifted_characters_use_the_modified_key` | Shift+a → `A`、Shift+1 → `!`;方向键用 `key`;⌘ 与 KeyReleased 无字节 |
| 2 | `keys_are_dropped_when_the_panel_is_hidden_unfocused_or_closing_or_has_no_session` | 五种丢弃条件都不写 PTY;反面对照写 1 次 |
| 3 | `a_pointer_press_inside_the_terminal_focuses_it_and_outside_unfocuses` | 矩形内 `focus=true`,矩形外 `false` |
| 4 | `captured_key_events_never_reach_the_terminal` | `Status::Captured` → `None`,`Ignored` → `Some(AgentKey)` |
| 5 | `command_c_copies_only_with_a_selection_and_otherwise_does_nothing` | 无选区 → `None`;有选区 → `Copy("hello")` |
| 6 | `command_v_reads_the_clipboard_and_pastes_with_bracketed_paste_when_the_app_enabled_it` | ⌘V → `Paste`;开启 bracketed paste 后写入 `ESC[200~…ESC[201~` |
| 7 | `dropped_file_writes_its_quoted_path` | `/tmp/a b.txt` → `'/tmp/a b.txt' ` |
| 8 | `ime_commit_writes_the_text_and_clears_the_preedit` | 写入 `你好` 的 UTF-8,`preedit == None` |
| 9 | `preedit_is_cleared_when_focus_is_lost_or_the_panel_is_hidden` | 点外 / 不可见 / 旧项目迟到消息都清空 |
| 10 | `input_goes_to_the_active_session_of_the_focused_project_only` | 只写当前项目的活动会话 id |

测试名守恒:1b 之后 465 + 新增 10 = 475(干净快照实测见下)。

## 验证(最终代码)

| 项 | 命令 | 结果 |
| --- | --- | --- |
| 基线核对 | `git rev-parse --short HEAD`(开工时) | `438c42e` |
| 桌面单包 | `cargo test -p digger-desktop` | 退出 0;312 通过 / 0 失败(+ smoke 1) |
| clippy | `cargo clippy --workspace --all-targets` | 退出 0;仅基线 `block v0.1.6` 1 条 |
| 格式 | `rustfmt --edition 2024 --check` 对本期改动文件 | 干净 |
| 干净快照 | `git archive 5a9e346` → 独立临时目录(名称与 `DIGGER_DATA_DIR` 都不含 "digger",临时 `HOME`)、自己的 `target` | 见下 |

### 干净快照实测

快照 `/private/tmp/s6d2-t5-clean.l2dQKF`(不含 "digger"),数据根 `/private/tmp/s6d2-t5-data.Acxtji`,临时 `HOME` `/private/tmp/s6d2-t5-home.MTKbhA`,自己的 `target`,归档自 `5a9e346`:

| 命令 | 退出码 | 结果 |
| --- | --- | --- |
| `cargo metadata --locked --offline` | 0 | 锁文件可复现 |
| `cargo test --workspace --no-fail-fast` | 0 | **475 通过 / 0 失败 / 0 ignored**(39 + 17 + 12 + 7 + 312 + 1 + 35 + 11 + 11 + 30) |
| `cargo clippy --workspace --all-targets` | 0 | 仅基线 `block v0.1.6` 1 条 |

## 待用户(真机项;没有用猜测代替)

需要键盘 / 输入法的项无法在无头环境代替,请按下列清单在沙箱(`DIGGER_DATA_DIR` + 临时 `HOME`,不碰真实数据与常驻 `diggerd`)里逐条试:

1. 打开项目 A 的 Agent 面板,输入 `echo hi`。
2. 方向键调出历史命令。
3. `sleep 100` 后 Ctrl+C 打断。
4. `vim` 进入/退出,确认重绘与按键正常。
5. 粘贴多行文本(应用开启 bracketed paste 时不应自动缩进错乱)。
6. 选中一段文本后 ⌘C 复制;无选区时 ⌘C 不产生副作用。
7. 从 Finder 拖一个带空格的文件路径进终端。
8. 中文输入法输入一句话。
9. **风险 1:** 组字期间观察终端是否收到拼音字母(理论上已被 `preedit.is_some()` 抑制;若仍有双重输入,请记录现象)。
10. **风险 2:** 在「项目」面板输入框里打字,终端不得收到按键;反之亦然。
11. **风险 3:** 开项目 A/B 各一个会话,输入只进当前项目的活动会话。
