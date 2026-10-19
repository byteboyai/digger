# S1 窗口骨架:参照 Dozer 的窗口结构,抽成 bytehost 共享 crate

- 日期:2026-10-14
- 状态:已实现(待真机验收,见计划 Task 15);2026-10-14 读完 Dozer 实现后修订(见 §8)
- 路径:架构级(新增共享 crate,重组 Digger 外壳里面板的组织方式)
- 相关:ADR-001(外壳与编辑器宿主边界)、ADR-011(bytehost 应用宿主)、ADR-013(Markdown 面板)、
  Dozer 的 `docs/dozer-v2/dozer-v2-面板独立性与服务化要求.md` 与 `docs/dozer-v2/digger-bytehost-integration/ownership.md`

## 1. 用户意图(原话归纳)

Digger 的窗口结构要和 Dozer **一模一样**。Dozer 里已有的 todo、agent、群聊、会话、用量面板要**重用**,并且 Dozer 里修改这些面板时要**同步**到 Digger。project 和 files 面板同样与 Dozer 共享(2026-10-15 更正:原先是 Digger 自有)。

### 假设(请纠正)

- “一模一样”指**窗口结构与交互**(顶栏 + 左右图标栏 + 左右面板区 + 分隔线 + 配对视图 + 放大态 + 底栏),不要求 Digger 暂时就具备 Dozer 的所有面板和业务浮层。
- “同步”指共享同一份代码(共享库),而不是两边各存一份靠人工对齐。

## 2. 拆分

这个需求太大,一份 spec 装不下。拆成五个子项目,各自走 spec → 计划 → 实现:

| # | 子项目 | 说明 |
|---|---|---|
| **S1** | **窗口骨架(本文)** | 共享骨架 crate + Digger 接入;现有四个面板装进去 |
| S2 | 共享面板:用量 | 已是共享库 `dozer-panel-usage`;接进 S1 的“列表 \| 内容”配对 |
| S3p | Project 实体、注册表、共享记忆 | 前置于 S3b;Dozer 的 Project 提成共享 `dozer-store-project`,Digger 用它打开项目文件夹并在顶栏显示页签(见 `2026-10-15-project-host-design.md`,ADR-014) |
| S3 | 共享面板:todo | Dozer 先抽成库(约 4.3k 行),两边引用 |
| S4 | 共享面板:对话、群聊、agent | 同上;agent 与外壳纠缠最深 |
| S5 | Project 面板 + files | Project 身份、注册表、共享记忆已由 S3p 提供(ADR-014);S5 只做面板界面,从 Dozer 抽库共享。S5a(Project 面板)、S5b-1(文件树,单栏)已完成;S5b-2(预览引擎)P1–P3 在 Dozer 已完成,Digger 的配对视图与接线(P4)待做 |

S1 是其余所有子项目的前提:S2–S4 的面板要挂在它的配对视图里,S5 要把它的持久化拆成按项目一份。

## 3. 已定的决定

1. **共享方式**:把 Dozer 的 chrome 抽成共享 crate,放进 bytehost;Digger 先接入,Dozer 在 S1 里不动,之后再迁移。
2. **做法 A**:以 `PanelId` 为键重写出共享骨架,不带 Dozer 的封闭枚举 `PanelKind`。(否决:B 把 chrome 原样搬成以 `PanelKind` 为泛型参数的库;C 只共享数据与几何、控件各写各的。)
3. **顶栏**:结构与 Dozer 一致,项目页签行保留但内容留空,“＋”置灰并提示 Project 尚未接入。
4. **边界**:骨架状态机在共享 crate 里,产品只实现 `ShellHost`。

## 4. 设计

### 4.1 边界与分工

新增 crate `bytehost-chrome`(在 `../bytehost` 仓库),依赖 `bytehost-core`、`bytehost-desktop`、`byteui`、iced。

| 单元 | 职责 | 依赖 |
|---|---|---|
| `ShellModel` | 无界面的骨架状态与 `update(ShellMsg)` | `PanelId`、`PaneStore` |
| `geometry` | 纯函数:窗口 + 模型 → 各区域矩形 | 无 iced |
| 控件 | 图标栏(含拖拽换栏换序)、面板区外框、分隔线、配对视图、放大浮层、顶栏外壳 | iced、byteui |
| `ShellHost` trait | 产品实现:面板清单(`PanelId` + 标题 + 图标)、渲染某面板 body 的回调、顶栏各槽位 | 由产品提供 |

bytehost 已有、直接复用:`PanelId`、`PaneStore` 与 `apply_pane_split_drag`、`route_lifecycle`(Digger 一侧)。

**数据流**:产品的 `update` 把 `ShellMsg` 交给 `ShellModel`;产品的 `view` 调 `chrome::view(&model, &host)`;骨架画 rail、分隔线、外框,面板 body 由 `host` 回调画;产品摆原生 webview 时问 `geometry` 要内容矩形。

**不进共享 crate**:面板自己的状态与消息、项目页签内容、业务浮层、Dozer 的 `webview_geometry.rs` 里按面板写的细节。Dozer 的 `PanelKind` 不进来,Dozer 迁移时自己做 `PanelKind ↔ PanelId` 映射(它的描述符里已有 `stable_id()`)。

### 4.2 `ShellModel`

**持久状态**(键全是 `PanelId`):
- 两条图标栏:`[Vec<PanelId>; 2]`,`[0]` 左、`[1]` 右,各为有序的 `PanelId` 列表。(骨架用自己的 `shell.json` 格式,**不用** `bytehost_core::Layout`:骨架恒为左右两栏,而 `Layout` 是 N 栏、栏宽语义也不同。)
- 每侧选中的 `PanelId`、每侧收起态。
- 左面板区宽度(像素);右区恒为剩余空间,只有一条 LeftRight 分隔线。
- `PaneStore`:每个面板的 `split` 与 `collapsed`。

**瞬态**(不落盘):放大的是哪一侧、聚焦区、图标栏拖拽、分隔线拖拽。

**`ShellMsg`**:选中面板;收起 / 展开某侧;放大 / 退出放大;图标栏拖拽(开始、悬停、结束);分隔线拖拽(开始、移动、结束);窗口缩放。`update` 返回“状态是否变化、是否需要落盘”,不直接写文件。

**不变量**(由 `ShellModel` 保证):
- 左宽总被夹在“当前窗口宽度下合法”的区间,上下限都给对侧留够最小区宽。这是**唯一**一处夹取,渲染、几何、拖拽都走它(Dozer 因各写一份夹取踩过坑:窗口拖窄后旧宽度把右区挤成 0 宽)。
- 选中的面板必须在对应 rail 里;rail 里的面板必须在清单里;任一 rail 不允许被拖空。

**持久化**:
- 一个 JSON 文件,字段全部 `#[serde(default)]`,缺字段补默认而不是整份失败。
- 内容:窗口大小(全局)、rail 布局、左宽、每侧选中面板、`PaneStore`。Digger 暂时全局一份;接口按 scope id 留好,S5 有了 Project 再把后几项拆成按项目一份。
- 读盘后统一消毒:顶层不是对象(serde 会把 `[1,2]` 这样的数组按"位置序列"塞进结构体,把损坏文件当成已恢复)、分割比例不在合法区间、左宽非正、窗口尺寸为 0 或小于下限、两侧都收起,都回落到合法值;任一 rail 为空则整体回落默认布局。**清单里有而文件里没有的面板回到它的默认栏末尾**(与 Dozer 有意不同:Dozer 对布局"不做部分修复",因此为"新增面板"专门写了迁移函数;这里新版本新增面板不会重置用户布局)。更高 schema 版本的文件沿用 Digger 现有做法:内存里用默认值,拒绝覆盖原文件。

**行为来源**:图标栏点击、拖拽换栏、放大态进出、分隔线手感,spec 阶段**尚未逐条读过** Dozer 的 `chrome/rail.rs`(1487 行)和 `app/update.rs` 里的对应分支。实现计划的第一步就是逐条读出并整理成行为表,Dozer 现有测试移植过来作验收,而不是凭印象重写。

### 4.3 几何与原生 webview

**一份度量,两处使用**:控件和几何函数读同一份度量,不允许各自写常量。度量有两个来源:
- `byteui::theme::geometry`(已有,Dozer 早已把图标栏宽、分隔线宽、最小区宽、顶栏高度、放大浮层内边距等迁到这里);
- **`byteui::theme::region` 的外壳区域样式(S1 新增)**:顶栏、左右图标栏、左右面板区、放大浮层、底栏的背景 / 边框 / 内边距 / 间距 / 外边距。Dozer 里这部分现在在 `dozer-app` 自己的 `theme::region`,数据来自 `assets/theme/workspace.json`,不在 byteui。S1 把这 7 个外壳区域的数据搬进 byteui,**数值与 Dozer 的 workspace.json 逐项相同**,用一个跨仓库的一致性检查脚本守住;Dozer 的 `region.rs` 在 S1 里不动,等 Dozer 迁移时再改为引用 byteui。

**纯几何函数**(无 iced、无窗口,可直接单测):
- `layout(window, &model, &metrics) -> ShellRects`:顶栏、两条图标栏、两个面板区、分隔线、放大态下被放大那一侧的矩形。
- `pair_rects(panel, &rects, &pane_store) -> (first, second)`:配对里的两块,按 split 与收起态算,收起时内容拿满。
- `content_rect(panel, &rects) -> Option<LogicalRect>`:面板当前的内容矩形;面板没被选中、所在区收起、被另一侧的放大态盖住时返回 `None`。

**原生 webview 的规则**,统一放在骨架里:
- 只有当前选中面板所在的、未收起的区可见;放大态下只有被放大那一侧可见;其余一律隐藏(沿用 Digger 已验证的“隐藏而不销毁”)。
- 产品拿到 `content_rect` 后,减去自己面板头部的高度(例如编辑器顶栏的 56px)再摆放。头部高度是面板自己的事。
- 窗口缩放、拖分隔线、收起、放大、切面板,都会改变 `content_rect`;每次 `ShellModel` 变化,产品重新同步矩形与可见性,与现在的 `sync_native` 做法一致,只是矩形来源换成骨架。

**防错位**:控件树里左区 `Fixed(left_width)`、右区 `Fill`(与 Dozer 相同),几何函数按同一规则算;再用真实 NSView 探针验证(见 §6)。

### 4.4 Digger 一侧

- 四个现有面板(信息、应用、用量、编辑器)成为 `ShellHost` 的 body 回调,各自仍是自包含模块(markdown 面板的独立性不变)。`ActivePanel` 枚举与顶部标签按钮删除。
- **默认布局**(沿用 Dozer 的分工:左区内容型、右区辅助型;这是提议,不是 Dozer 的既有数据):
  - 左 rail:编辑器、信息;右 rail:应用、用量。
  - 启动选中:左 = 编辑器,右 = 用量。左宽默认 640,与 Dozer 一致,可拖。
- **S1 做**:顶栏结构(项目页签行、置灰的“＋”、首页入口位、窗口控制)、两条图标栏、两个面板区、分隔线、配对视图控件、放大态、底栏占位槽、布局持久化。
- **窗口装饰**(S1 做):与 Dozer 一致,透明标题栏 + 全尺寸内容视图,顶栏取代系统标题栏;macOS 红绿灯按钮挪进顶栏内垂直居中;双击顶栏空白处缩放窗口。
- **放大态**:Dozer 的生产代码里目前**没有任何入口**把放大态置为 `Some`(只有退出路径和测试)。S1 照搬状态、浮层、几何和退出路径,入口只做成一条 `ShellMsg::Maximize(side)` 消息,不加任何界面按钮,与 Dozer 现状一致。
- **S1 不做**:首页落地页内容、项目页签内容、底栏内容、各类业务浮层(设置、搜索等)。保留结构位置,内容随后续子项目补。
- 配对视图控件在 S1 交付并用测试夹具验证;Digger 里真正的“列表 \| 内容”配对要等 S2 起的面板才出现。

## 5. 交付方式

涉及三个仓库:

| 仓库 | 改什么 | 隔离方式 |
|---|---|---|
| `../byteui` | 新增 `theme::region`(外壳区域样式) | 工作区干净,可在 `main` 上建分支 |
| `../bytehost` | 新增 crate `bytehost-chrome` | **主工作区有他人未提交的 i18n 改动(十几个文件)**,不得混入、不得让 Digger 依赖它;改用 `git worktree` 从 `v0.2.0` 切出独立分支 |
| `digger` | 接入、删除标签页、窗口装饰、自检 | 本仓库 |

- **开发期依赖**:Digger 用 `[patch]` 把 bytehost 与 byteui 的 git 来源指向上面两个 worktree 的路径,使整个依赖图只有一份 `bytehost-core` 和一份 byteui(两份会让 `PanelId` 类型不兼容,也会让 byteui 的全局主题状态分裂,用量面板的主题会错)。`[patch]` 段是开发期临时配置,在最后一个任务里随 tag 的出现而移除。
- 完成后,bytehost 与 byteui 各打新 tag,Digger 改为固定 tag,与现在的方式一致。
- **Dozer 在 S1 里不动**。Dozer 迁移是单独的后续工作。
- 在 bytehost、byteui 仓库只做**本地提交**,不推送、不打 tag;推送与打 tag 由用户决定。

## 6. 验收

1. **模型与几何单元测试**:夹取、rail 不能拖空、选中面板必须在 rail 里、损坏布局被消毒、窗口缩窄、放大态;图标栏拖拽逻辑用 Dozer 现有测试移植对照。
2. **Digger 布局文件**:往返、更高 schema 版本不覆盖,沿用现有测试并适配。
3. **真实窗口自检**:把现有生命周期自检扩成骨架版 —— 选面板、收起、放大、拖分隔线、缩放窗口,每步读真实 NSView 的可见性 / 位置 / 大小,与**独立写的**期望值比较(期望值不调用被测的几何函数);并做破坏性验证(故意让某一步失效,对应检查必须失败)。现有 harness 自检 24/24 保持。
4. **门禁**:`scripts/check-host-deps.sh` 继续成立,`bytehost-chrome` 只允许 `digger-desktop` 依赖。
5. **视觉一致无法靠测试断言**:分别截取 Dozer 与 Digger 的窗口,由用户对照确认“窗口结构一模一样”。

## 7. 风险与未决

- **行为表尚未读出**(见 §4.2):图标栏与放大态的细节可能比设计里写的多;计划阶段先读,读完若发现范围显著变大,回到本 spec 修订,不带着假设往下做。
- **bytehost 是单独仓库,且主工作区正有他人的未提交改动**:见 §5。新 crate 在隔离 worktree 里做,最终合并回 bytehost 主线要处理与 i18n 改动的冲突,这不在 S1 范围内,由用户决定何时合并。
- **bytehost 现有依赖约束**:`bytehost-desktop` 的 iced 依赖是 `default-features = false`(不绑渲染器 feature,见其 Cargo.toml 的 R2 注释)。`bytehost-chrome` 沿用 byteui 的做法,控件返回 `Element<'a, M, iced_widget::Theme, iced_renderer::Renderer>`,不额外开渲染器 feature。
- **区域样式一致性**:byteui 里新增的 7 个外壳区域数据与 Dozer 的 workspace.json 是两份;S1 期间靠一致性检查脚本守住,Dozer 迁移时才合成一份。
- **S2–S4 的“同步”不在本文范围**:每个面板抽成共享库要先在 Dozer 里做,工作量大,各自单独立 spec。
- **默认栏位是提议**,可随时改默认布局而不影响结构。

## 8. 修订记录

2026-10-14,读完 Dozer 的图标栏 / 拖拽 / 放大 / 顶栏 / 窗口创建代码后修订,原因是发现与原 spec 不一致的事实:
1. 外壳区域样式在 Dozer 自己的 `theme::region` 里,不在 byteui;原 §4.3 写成"取自 byteui geometry"不准确。→ 新增 byteui `theme::region`(§4.3)。
2. `../bytehost` 主工作区有他人未提交的 i18n 改动;Digger 若直接依赖它,`AppSummary` 与 `AppSurfaceBuilt` 的形状已变,Digger 会编译失败。→ 用 worktree 隔离(§5)。
3. Dozer 的放大态没有生产入口。→ 只做消息入口,不加界面(§4.4)。
4. Dozer 的窗口是透明标题栏 + 全尺寸内容视图,红绿灯挪进顶栏。→ 纳入 S1(§4.4)。
5. Dozer 的拖拽事件由它自己的 winit 层追踪;Digger 用 iced 运行时,计划里改用 iced 的 `event::listen_with` 追踪光标与松开。
6. 落盘格式:自己的 `shell.json` DTO(§4.2),不用 `bytehost_core::Layout`;缺失面板回到默认栏(§4.2)。原因见 §4.2。
7. 顶栏拖窗保护(Dozer 覆写 `mouseDownCanMoveWindow` 的那段 unsafe 代码)**不在 S1**:S1 的顶栏没有任何可交互控件,等顶栏有了项目页签(S5)再移植并加真实窗口验证。
8. 计划阶段发现原生视图的大小是取整的,真实窗口自检的比较容差取半个像素(够吸收取整,又抓得住 1 像素的几何漂移)。
