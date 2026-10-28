# 会话面板遗留缺口

日期:2026-10-27 起草,2026-10-28 收尾更新。收尾计划:`dozer/docs/superpowers/plans/2026-10-27-s7-s8-closeout.md`。

## 已解决

### 1. 删除项目不清理该项目的会话 —— 已裁决 A 并完成

裁决:只清 `digger.db` 里的副本。完成(Digger `338c1ea`):项目从注册表移除后,Digger 自己调 `TranscriptService::delete_project_transcripts(cwd)`;不传库的 `transcript` 参数(那条路径会顺带删 agent 缓存目录)。

**更正:** 原先写的「Dozer 里该调用会同时删除 home 下的 transcript 原文件」不准确。`delete_project_transcripts` 只删库里的 conversations/turns 行,不碰任何原文件;删 agent 缓存目录是 Dozer Project 面板 `spawn_delete_project` 在带 `transcript` 且范围不是 DozerOnly 时另做的。所以 D1=A **不需要** Dozer 新接口,只需 Digger 侧接线。

### 2. 总结生成 —— 已裁决继续关闭

标题回落到首条发言或会话自带标题。开通需要打开 `diggerd` 的总结功能(调外部 LLM/Agent,有费用),需要时再单独立项。

### 历史导入失败明细 —— 已完成

Dozer 新增 `BackfillReport`/`BackfillFailure` 与 `TranscriptService::backfill_project_transcripts_report`(wire 只追加,旧端互通)。Digger(`cc9f61b`)改调它,提示条列出前 5 个失败文件与原因,其余只给数量。

## 仍然未决

### 3. 跨仓库迁移(X1)

页签部件(`agent_tabs.rs`)、菜单弹层(`menu_overlay.rs`)和 `review_native` / `native_host` 的 wry 样板迁入 byteui / bytehost。涉及两个仓库发版与双方重钉,单独排期。

### 4. 搜索框右键菜单(G1)

`TextInputMenuOpen` 在 Digger 仍是空操作。Dozer 的做法是原生菜单 + 合成 ⌘C/V/X/A 键盘事件回灌给输入框(含先聚焦目标),Digger 要接同样一套事件机制,并需中文输入法下人工验证。建议单独立项。
