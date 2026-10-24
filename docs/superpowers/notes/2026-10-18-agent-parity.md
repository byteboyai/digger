# Agent 面板 Digger ↔ Dozer 对照(S6d-3)

日期:2026-10-23。Digger 侧用 `cargo run -p digger-desktop --example agent_gallery -- --state <name>`(沙箱 `DIGGER_DATA_DIR` + 临时 `HOME`,不连 `diggerd`)。Dozer 侧需要用户在真实 Dozer 里摆同一状态后截图;下表与差异清单在拿到 Dozer 截图后逐条填写。

## 状态与摆法

| state | Digger 画廊 | Dozer 侧怎么摆 |
| --- | --- | --- |
| `empty` | 无会话 | 打开 Agent 面板,关闭所有会话 |
| `one-shell` | 一个 OS Shell,Idle | 选择器选 OS Shell |
| `grouped` | Claude ×2(运行/待输入)、Codebuddy ×1(回合结束)、已退出的 shell | 同左,让 hook 上报对应状态 |
| `active-card` | 选中 Claude 卡片,有模型/模式/工作行/分支 | 同左 |
| `many-tabs` | 12 个页签,翻页窗口化 | 同左 |
| `narrow` | 760×600,页签条接近满 | 把窗口缩到同样宽 |
| `overflow-open` | 8 个页签 + 溢出下拉 | 点页签条右侧 `⌄` |
| `picker-open` | 选择器浮层 | 点列表头部的 `＋` |
| `confirm-open` | 关运行中页签的确认框 | 点运行中页签的 × |
| `notice` | 顶部红字错误 + × | 制造一次 create 失败(如临时改名 diggerd) |
| `daemon-unavailable` | 不可用原因 + 重试 | 改名 diggerd 后启动/重试 |

## 差异清单

| 状态 | 差异 | 原因 | 处理 |
| --- | --- | --- | --- |
| 全部 | 悬停是阶跃(进入即满、离开即空),Dozer 是渐变 | 设计 D3 已接受 | 接受 |
| 全部 | 延迟提示精度 1 秒(Dozer 为逐帧) | 现有 1 秒 `Tick` | 接受 |
| — | 其余差异待用户提供 Dozer 截图后逐条填 | — | 待对照 |

## 技术债

页签部件(`agent_tabs.rs`)与菜单弹层(`menu_overlay.rs`)仍是 Digger 里临时复制的一份,长期应迁入 byteui;迁移前不要在两处各改一套。
