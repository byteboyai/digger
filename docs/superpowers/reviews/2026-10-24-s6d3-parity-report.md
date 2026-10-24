# S6d-3 页签对照验收与修差异报告

日期:2026-10-24。分支 `feat/agent-parity-fix`(worktree `../digger-s6d3-parity`),基线 Digger `main` `52df4b9`。Dozer 只读使用,没有修改它的代码或真实数据。

## 提交

- `d01c841` fix(agent): 页签/溢出菜单与 Dozer 对照后修差异

未合并 `main`、未打 tag、未推送、未发布。库未改。

## 做法

- **Digger:** 11 个状态逐次运行 `cargo run -p digger-desktop --example agent_gallery -- --state <name>`,隔离 `DIGGER_DATA_DIR` 与临时 `HOME`;用 Orca 的 computer-use 按 pid 截图,存 `docs/superpowers/notes/parity-shots/<state>-digger.png`。
- **Dozer:** 用安装包的**副本** `/private/tmp/ParityDozer.app`(只改 bundle id 为 `com.byteboy.dozer.parity` 并 ad-hoc 重签),隔离 `HOME`;用 `dozerd` 的 UDS 预置项目与会话(`open_project` / `create_session`),写 `open_projects.json`,启动后直接截图。**没有点真实 Dozer 的界面,也没有碰真实 HOME。**
- Dozer 的自绘按钮无法用 Orca 坐标点击触发(只能 hover 出提示),所以需要点击才能摆出的 4 个状态标"未对照",没有编造。

## 对照结论

状态表、13 项度量/颜色对照与"未对照"原因都在 `docs/superpowers/notes/2026-10-18-agent-parity.md`。**没有遗留"待修"项。**

修掉的差异(修前 → 修后):

| # | 项 | 修前 | 修后(与 Dozer 一致) |
| --- | --- | --- | --- |
| 1 | 页签标题字体 | 默认字重 | 系统默认字体 + `Weight::Medium`(`top_bar_font()`) |
| 2 | 状态点 ↔ 标题间距 | 6 | 4 |
| 3 | 悬停关闭按钮 | 胶囊不浮现、× 单独 | `max(title_hover, close_hover)` 同时驱动胶囊与 × |
| 4 | 延迟提示样式 | 样式加在气泡 container | 加在 `Tooltip`(同 Dozer) |
| 5 | 溢出菜单非选中行标题 | 恒 cream | dim |
| 6 | 溢出菜单行前缀 | 无状态点 | 补 `status::dot(entry.dot)` |
| 7 | 溢出菜单行 hover | 无高亮 | `tab_hover` 底色 |
| 8 | 列表折叠按钮 | 裸 `button()`(iced 默认蓝色实心块) | `icon_button_entry`,命中区 `icon_size::row()+6`,自带 tooltip |
| 9 | overflow 行 `>` 列 | 固定宽 | 已有,保持 |

另外修了**画廊自身**两个问题(不是产品缺陷):覆盖层在项目状态入表之前打开导致 overflow/picker 截图不出现;列表方向与真实产品相反(现在终端左、列表右)。

## 已接受差异(不修)

1. 悬停是阶跃,Dozer 是渐变。
2. 延迟提示精度 1 秒,Dozer 逐帧。
3. 覆盖层状态用 `picker`/`overflow` 两个互斥字段,不是单个枚举。

## 未对照 / 观察

- **未对照:** `empty`(Dozer 项目打开后自动建 shell,摆不出 0 会话)、`overflow-open`、`picker-open`、`confirm-open`、`notice`(都需要点击自绘按钮,Orca 点击只能 hover)。`grouped` 只对照到"空闲/已退出",Running/Awaiting/TurnEnded 三态没有 hook 驱动。
- **数据差异:** 画廊假会话带模型/模式/工作内容,Dozer 预置会话没有,卡片文案不同不是 chrome 差异。
- **产品差异(不在页签范围):** Dozer 守护进程断开是底部 toast + "会话已结束";Digger 是面板内原因 + 重试。Dozer 终端底部有"Agent 上下文/修改历史",Digger 一期没有。

## 测试

新增 3 条(全在 `agent_tabs`):
`tab_title_font_is_medium_like_dozers_top_bar_font`、`hovering_the_close_button_also_shows_the_capsule`、`inactive_overflow_row_titles_are_dim_and_active_is_cream`。

基线 536 + 3 = **539**(干净快照实测见下)。

## 验证(最终代码)

| 项 | 命令 | 结果 |
| --- | --- | --- |
| 基线核对 | `git rev-parse --short main` | `52df4b9` |
| 桌面单包 | `cargo test -p digger-desktop` | 退出 0;362 通过 / 0 失败(+ smoke 1) |
| clippy | `cargo clippy --workspace --all-targets` | 退出 0;仅基线 `block v0.1.6` |
| 格式 | `rustfmt --edition 2024 --check` 对本任务改动文件 | 干净 |
| 干净快照 | `git archive d01c841` → 独立目录(名称/数据目录不含 "digger") | 见下 |

### 干净快照实测

`/private/tmp/s6d3q-clean.yCGLat`(不含 "digger"),数据根 `/private/tmp/s6d3q-data.YEzI8L`,临时 `HOME` `/private/tmp/s6d3q-home.gnYczz`,自己的 `target`,归档自代码提交 `10c2cb8`(之后的 `b32108c` 只删了一张误标截图与改报告,不含代码):

| 命令 | 退出码 | 结果 |
| --- | --- | --- |
| `cargo metadata --locked --offline` | 0 | 锁文件可复现 |
| `cargo test --workspace --no-fail-fast` | 0 | **539 通过 / 0 失败 / 0 ignored** |
| `cargo clippy --workspace --all-targets` | 0 | 仅基线 `block v0.1.6` 1 条 |

注:交接单提到的偶发失败 `apps::tests::closing_during_startup_waits_and_releases_the_root` 本轮没有出现。

## 待用户

- 需要点击才能摆出的 Dozer 状态(overflow / picker / confirm / notice)如果想补对照,需要人工在 Dozer 里操作并截图;Digger 侧截图已在仓库里。
- 发布、压力、同机并存、中文输入不在本任务内,未做。
