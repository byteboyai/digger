# Agent 面板 Digger ↔ Dozer 对照(S6d-3,2026-10-24)

Digger 侧:`cargo run -p digger-desktop --example agent_gallery -- --state <name>`(隔离 `DIGGER_DATA_DIR` + 临时 `HOME`,独立窗口,不连 `diggerd`)。截图在 `docs/superpowers/notes/parity-shots/<state>-digger.png`。

Dozer 侧:用 `/Applications/Dozer AI Coder.app` 的**副本** `/private/tmp/ParityDozer.app`(只改 bundle id 为 `com.byteboy.dozer.parity`、ad-hoc 重签,不碰安装包),隔离 `HOME`;项目/会话通过 `dozerd` 的 UDS 预置(`open_project` + `create_session`),`open_projects.json` 直接写项目 id。截图在 `parity-shots/<state>-dozer.png`。**没有修改 Dozer 的代码或真实数据。**

## 状态对照

| state | Dozer | 结论 |
| --- | --- | --- |
| `empty` | 项目打开后 Dozer 自动新建一个 shell,无法摆出"Agent 面板 0 会话" | 未对照 |
| `one-shell` | 已对照 | 页签/列表/终端布局一致;卡片文案差异是数据(见下) |
| `active-card` | 已对照 | 选中卡片外观一致 |
| `grouped` | 只摆到 4 个会话、状态全是"空闲"(没有 hook 驱动 Running/Awaiting/TurnEnded) | 部分对照:四态颜色未对照 |
| `many-tabs` | 12 个会话;Dozer 右栏可用宽度较窄,只显示前 4 个页签 | 已对照(页签窗口化) |
| `narrow` | `layout.json` 预置 900×700 | 已对照(尺寸与 Digger 760×600 不同,只看结构) |
| `overflow-open` | 无法用坐标点击触发 Dozer 的自绘按钮(Orca 只能 hover 出提示) | 未对照 |
| `picker-open` | 同上 | 未对照 |
| `confirm-open` | 需要 Running 状态 + 点 ×,同上无法点击 | 未对照 |
| `notice` | 需要制造一次 create 失败,无法点击 | 未对照 |
| `daemon-unavailable` | 已对照(启动后 kill 隔离的 dozerd) | 设计不同:详见下 |

## 差异清单

| # | 项 | Dozer 值 | Digger 修复前 | 处理 |
| --- | --- | --- | --- | --- |
| 1 | 页签标题字体 | `top_bar_font()`:系统默认字体 + `Weight::Medium` | 默认字重 | 已修:`agent_tabs::tab_font()` 用 Medium |
| 2 | 状态点与标题间距 | `tab_label` 的 `row.spacing(4)` | 6 | 已修:4 |
| 3 | 悬停 × 时的胶囊 | `hover = max(hover_t, close_hover_t)` 同时驱动胶囊与 × 显形 | 只按标题 hover 驱动胶囊,× 单独 | 已修:`combined_hover` |
| 4 | 延迟提示 | `Tooltip::style(tooltip_bubble_style)`;气泡本身只设字色/内边距 | style 加在气泡 container 上 | 已修:与 Dozer 同位置 |
| 5 | 溢出菜单非选中行标题色 | `tab_label(active, hover=0)` → `dim` | 恒 `cream` | 已修:`title_color(active, 0.0)` |
| 6 | 溢出菜单行前缀 | 状态点(`entry.prefix`) | 没有状态点 | 已修:补 `status::dot(entry.dot)` |
| 7 | 溢出菜单行悬停 | `entry.hover_t` 铺 `tab_hover` 底 | 恒 0,无高亮 | 已修:行 hover 上报 + 容器底色 |
| 8 | 列表折叠按钮 | `icons::icon_button_entry`,命中区 `icon_size::row()+6`,自带 tooltip | 裸 `button()`,被 iced 默认样式画成蓝色实心块 | 已修:改用 `icon_button_entry` |
| 9 | 页签条可用宽度 | 终端块宽 − 72 | 同(常量 `TAB_BAR_RESERVE=72`) | 无差异 |
| 10 | 页签内边距 | 左 10 / 右 4 / 上下 0 | 同 | 无差异 |
| 11 | 选中/悬停容器 | card 底 + 1px border + 圆角 6 / tab_hover + 圆角 6 | 同 | 无差异 |
| 12 | 溢出菜单宽度/高度 | `clamp(win.w−anchor−24, 220, 560)`、最大高 320、行内边距 4、行距 2、固定 `>` 列 | 同(按 `icon_size::scale()` 缩放,scale=1 时数值一致) | 无差异 |
| 13 | 列表/终端 pane 配色 | `agent_list_pane` padding 6/12/12/12 + gap 8;`terminal_pane` padding 8 + gap 4 | 同 | 无差异 |

## 已接受的差异(不修)

1. 悬停是阶跃(进入即满、离开即空),Dozer 是渐变。
2. 延迟提示精度 1 秒,Dozer 逐帧。
3. 覆盖层状态用 `picker` 与 `overflow` 两个互斥字段,而不是单个枚举。

## 其他观察(不在本任务范围,只记录)

- **卡片文案是数据差异**:画廊假会话带 `llm_model`/`permission_mode`/`last_activity`,Dozer 预置会话没有,于是 Digger 显示 `shell (Sonnet 5, default)`、Dozer 只显示 `shell`;不是 chrome 差异。
- **列表宽度**:画廊固定 300,真实产品的列表宽度由配对 split 决定。
- **守护进程不可用**:Dozer 是底部 toast「与 dozerd 的连接中断,正在重连…」+ 页签内容被标「[会话已结束]」;Digger 是面板内整块原因 + 重试。这是 S6d-2 既定的产品措辞/壳,不是页签差异。
- **Agent 面板底部**:Dozer 有「Agent 上下文 · 0 项」与「修改历史」;Digger 一期没有,属于后续面板内容。
- **Dozer 截图局限**:Orca 对 Dozer 自绘按钮的坐标点击只能 hover(提示气泡出现)不能触发;因此 overflow/picker/confirm/notice 四项未对照,没有编造。
- **技术债(已解决,2026-10-28)**:页签部件与菜单弹层已迁入 byteui 0.5.0,Digger 的 `agent_tabs.rs`、`tab_chrome.rs`、`menu_overlay.rs` 缩成接线。见 ADR-015 与 `dozer/docs/superpowers/plans/2026-10-28-x1-cross-repo-migration.md`。
