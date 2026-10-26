# S3b-2 Digger MCP 待办工具验收清单

日期：2026-10-25
分支：`feat/mcp-todos`
前提：在**新安装的** Digger 版本上验收；Agent 会话必须**重开**（agent 只在会话启动时读取一次 MCP 工具清单）。

## 环境

- 沙箱 Digger：独立 `DIGGER_DATA_DIR` + 临时 `HOME`。
- 不要使用日常 `/Applications/Digger.app` 的数据根，也不要杀用户常驻的 `diggerd`。
- 建议先建一个测试项目并打开待办面板，确认面板本身可用。

## 清单

1. [ ] Agent 面板新开一个 Claude 会话，让它“列出我的待办”：返回内容与待办面板一致。
2. [ ] 让它“新增一条待办：xxx”：待办面板约 1 秒内出现该条；面板收起时新增，再展开后也在。
3. [ ] 让它“把 xxx 标记完成”：面板里该条变为已完成。
4. [ ] 让它改一条任务的文字：面板里的文字更新。
5. [ ] 对 Codex、其它已接入的 agent 各重复 1–4 中的任意两项。
6. [ ] Agent 的 MCP 状态页：`digger` 显示可用，工具数比升级前多 4（`list_todos` / `add_todo` / `toggle_todo` / `edit_todo_text`）。
   - [ ] 若 `dozer` 一项显示启动失败：属于 S6e 设计 §2 的已知并存现象（`dozer-mcp` 缺 `DOZER_SESSION_ID`），与待办工具无关；同时确认 `digger` 本身可用。
7. [ ] 会话目录是一个没登记的项目：让 agent 调 `list_todos`，得到一条可读的错误而不是卡住。
8. [ ] 已收紧的行为（libs-2026-10-26a 起）：
   - [ ] 让 agent 对别的项目的任务 id 调 `toggle_todo` / `edit_todo_text`，得到「不属于当前项目」的可读错误，任务不变。
   - [ ] 让 agent 调 `add_todo` / `edit_todo_text` 传空串或纯空白，得到「任务文字不能为空」的错误，不产生空任务、旧文字不变。
   - [ ] 升级前已开着的 Agent 会话看不到新工具。
   - [ ] 文本框右键菜单、详情弹窗、写失败 Toast 仍按 S3b-1 的已知缺位处理。

## 通过标准

1–7 全部符合预期；第 8 项的两处拒绝符合预期，其余仍是已知缺位、不是本次改动引入的回归。
