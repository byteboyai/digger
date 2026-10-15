# S2 用量面板接入"列表 | 内容"配对视图

- 日期:2026-10-15
- 状态:待评审
- 路径:架构级(给共享骨架 `bytehost-chrome` 加一条消息,并让 Digger 里第一个面板用上配对视图)
- 相关:S1 窗口骨架 spec(`2026-10-14-window-shell-design.md`)、ADR-011、ADR-013

## 1. 用户意图

S1 把 Dozer 的窗口结构搬进了 Digger,但配对视图控件只用测试夹具验证过,Digger 里还没有任何面板在用。S2 让「用量」成为第一个真正的配对面板,行为与 Dozer 一致:右侧面板区里左边是统计内容,右边是 agent 筛选列表。

约束(用户已确认):
- 用量面板继续用共享库 `dozer-panel-usage`,Dozer 改它时要能同步到 Digger。
- **S2 不升级这个依赖。** Digger 钉的 `67ab0e6` 比 Dozer 主线旧(之后 Dozer 加了 i18n:`content_pane` 多了 `locale` 参数)。所有共享面板的升级留给一个统一的"同步到 Dozer 主线"子项目,它依赖 Digger 的语言来源、bytehost i18n 的发布、byteui 版本线理清这几件事,目前都没满足。
- 适配层里调用库的地方保持集中,之后同步只需补 `locale` 参数。
- 发布状态沿用 S1:继续用本地 worktree 和 `[patch]`,不推送、不打 tag。

## 2. 现状

- 共享库已同时提供 `view::content_pane` 和 `view::list_pane`;Digger 只渲染了 `content_pane`。
- 库的 `PanelDisplay::list_collapse_button` 在 Digger 里是占位按钮,发库自己的 `ToggleListCollapse`,没有真的折叠。
- S1 已交付:`PaneSpec` / `PaneStore` / `apply_pane_split_drag` 的复用、`pair_view` 控件、`ShellModel::pair_rects`、`shell.json` 里的 `panes` 持久化。`shell::rules()` 目前对所有面板返回 `None`。
- 用量的原生 webview(图表)位置由 `sync_native()` 按「面板内容矩形 − `USAGE_HEADER_HEIGHT`」摆放,不区分配对。

## 3. 配对规格(与 Dozer 逐项对齐)

Dozer 的用量配对:`pane_spec(Usage) = PaneSpec::new(default_list_first = false, collapsible = true)`,`usage_split` 是**列表**占配对宽度的比例,最小列表宽度用 `footer_min_width()`。

| 项 | Digger 取值 |
|---|---|
| 第一块 | agent 列表(`list_pane`) |
| 第二块 | 统计内容(`content_pane`) |
| `PaneSpec` | `PaneSpec::new(false, true)`:默认内容在前、列表在后;列表可拖窄收起 |
| 默认所在栏 | 右栏。镜像否;拖到左栏后镜像,变成"列表 \| 内容",由骨架几何算好 |
| `default_split` | byteui `default_split_ratio()` |
| `min_first_px` | 与 Dozer `footer_min_width()` 同一公式:`button_w * 2 + 6 + 16`,`button_w = icon_size::row() + 6 + font::label() * 4 + 16`。该函数在 Dozer 的项目面板里,Digger 在 `shell.rs` 里按同一公式自算,数值一致 |

这些放进 `shell::rules()`,仅对 `digger.usage` 返回规格,其余面板仍为 `None`。

## 4. 骨架增补(共享 crate,通用)

S1 的 `ShellMsg` 没有"切换配对收起"。Dozer 用 `ToggleListCollapse` 写 `usage_list_collapsed`。

在 `bytehost-chrome` 加:

- `ShellMsg::TogglePane(PanelId)`:翻转该面板在 `PaneStore` 里的 `collapsed`。
- `ShellModel::update` 处理它:面板没有 `PaneRules`(不是配对面板)时什么都不做、`persist: false`;否则翻转并返回 `persist: true`。
- 翻转时保留该面板已有的 `split`;面板在 `PaneStore` 里还没有条目时,用 `default_split` 建一条。
- `persist.rs` 无需改动(`panes` 已被存取与校验)。

这条消息对之后所有配对面板通用,不是用量专用。

## 5. 是否拼分栏:唯一判断

Dozer 的规则(`app/view.rs` 的 `PanelKind::Usage` 分支):
- 加载中或还没数据(`!has_agent_filter()`):内容栏独占全宽。
- 列表已收起:内容栏独占全宽,标题行末尾仍有按钮可展开。
- 否则拼分栏。

Digger 在 `product.rs` 里把它做成**一个函数** `usage_layout(&self) -> UsageLayout`,取值 `ContentOnly`(无数据)、`Collapsed`、`Paired`。`body()` 和 `sync_native()` 都读它,不各自再算一遍。`ContentOnly` 与 `Collapsed` 的原生矩形相同,区别只在内容栏标题行里是否显示"展开列表"按钮(库的 `content_pane` 已按 `collapsed` 参数处理)。

## 6. 原生 webview 位置

webview 只放在内容栏。矩形取自:

| `usage_layout` | 取哪一块 | 再减 |
|---|---|---|
| `Paired` | `pair_rects(...).second`(骨架已按镜像算好) | `USAGE_HEADER_HEIGHT` |
| `Collapsed` / `ContentOnly` | 面板内容矩形整个 | `USAGE_HEADER_HEIGHT` |

`USAGE_HEADER_HEIGHT` 仍是 S1 留下的估值,S2 不改它的值,也不替用户确认;列入验收清单目测校准。可见性规则沿用 S1(区收起、被另一侧放大盖住时隐藏)。

## 7. 展示适配

`adapters/usage.rs` 的 `PanelDisplay` 实现改动:

- `list_collapse_button`:按钮发 `Msg::Shell(ShellMsg::TogglePane(用量))`;按 `collapsed` 换图标和提示文字(tooltip 文案沿用库传入的两条)。
- `toggle_list_collapse` / `collapse_hover`:库要求实现,Digger 不再用它们驱动状态,返回无副作用的消息。
- 收起状态只存在骨架的 `PaneStore` 里,不在用量自己的状态里重复一份。
- 外框 `Border::default()`:区域外框已由骨架画。
- agent 图标继续回落公共 `Bot` 图标;Git 统计继续 `NoGitStats`。

## 8. 持久化

不新增文件。`shell.json` 的 `panes["digger.usage"] = { split, collapsed }`。读盘校验沿用 S1:split 不在 `[min_split_ratio, max_split_ratio]` 内的条目被丢弃回默认。

## 9. 明确不做

- 不升级 `dozer-panel-usage`(见 §1)。
- 不做 agent 品牌图标、不做 Git 统计。
- 不动编辑器、信息、应用三个面板,它们仍是单栏。
- 不加放大态的新入口。
- 不改 byteui、不改 Dozer。

## 10. 验证

**单元测试**
- `shell::rules()`:仅用量有规格;规格值(`PaneSpec`、`default_split`、`min_first_px`)。
- `bytehost-chrome`:`TogglePane` 翻转并 `persist: true`;非配对面板上无效且 `persist: false`;保留已有 split;无条目时用 `default_split` 建条;经 `persist` 往返。
- `usage_layout`:无数据、有数据、收起三种状态各自的取值。
- 产品:三种状态下原生矩形的期望值(独立手算);`body()` 与 `sync_native()` 的判断一致(同一函数,测试断言两处对同一状态给出一致结果)。

**真实窗口自检**(`scripts/run-editor-selftest.sh lifecycle`,读真实 NSView,期望值独立手算,不调骨架的几何函数)。在原有 11 步之后加:
- S12 拼分栏时用量 webview 的位置与大小。
- S13 收起列表后 webview 变宽并占满。
- S14 拖动配对分隔线后 webview 随之变化。
- S15 用量拖到左栏(镜像)后 webview 换到内容栏的新位置。

自检需要用量面板有数据才会拼分栏;用测试数据注入一份(与 harness 的做法同型),不依赖真实用量。

**验收**
1. 有数据时右区显示"内容 \| 列表",默认列表占 `default_split_ratio`。
2. 拖分隔线改比例;拖到列表窄于最小宽度时收起;点按钮可展开。
3. 收起与比例重启后保留;`shell.json` 被改坏不崩溃。
4. 用量拖到左栏后变成"列表 \| 内容"。
5. 无数据或加载中无分栏。
6. 真实窗口自检全绿,原有 11 步不退化。

**做不到、留给用户**
- 图表本身的渲染效果。
- `USAGE_HEADER_HEIGHT` 的目测校准。

## 11. 风险

1. **判断分叉**:`body()` 与 `sync_native()` 若各算一遍会不一致。由 §5 的唯一函数和 §10 的测试防。
2. **头部高度估值**:不准时 webview 会压住或离开头部。验收时目测。
3. **加载到有数据时布局跳变**:从单栏变分栏会跳。Dozer 同样如此,保持一致,不做过渡。
4. **共享骨架改动对 Dozer 的影响**:`TogglePane` 加在共享 crate 里。Dozer 尚未迁移到共享骨架,暂时没有影响。
5. **依赖滞后**:在统一同步完成前,Digger 的用量面板没有本地化。界面文字不受影响。

## 12. 子项目边界

S2 之后是 S3(todo)、S4(会话 / 群聊 / agent)、S5(项目 + files,与 Dozer 共享,需先解决 Digger 没有 Project 的问题),然后是统一的"同步到 Dozer 主线"。每个都有自己的 spec。
