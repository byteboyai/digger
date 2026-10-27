# 会话面板遗留缺口(待用户裁决)

日期:2026-10-27。以下三条不由实现者自行决定。

## 1. 删除项目不清理该项目的会话

Digger 删除项目时没有清理会话,`TranscriptService::delete_project_transcripts(cwd)` 未接入。Dozer 里该调用(`delete_project_transcripts_in(home, cwd)`)会**同时删除用户 home 下的 transcript 原文件**,所以接入是破坏性动作。

需要用户明确范围:

- A. 只清 `digger.db` 里的副本,不动原文件(需要 Dozer 侧提供只清副本的接口,涉及新 libs tag 和 Digger 重钉)。
- B. 连原文件一起删(与 Dozer 一致,不可逆)。
- C. 维持现状,不清理。

在裁决前,Project 面板的 `NoTranscript` 行为保持不变。

## 2. 总结生成在 Digger 的开通

本期「生成总结」关闭,标题回落到首条发言或会话自带标题。开通需要打开 `diggerd` 的总结功能,它会调用外部 LLM / Agent 命令行并产生费用。需要用户决定是否开通、默认开关与费用提示方式。

## 3. 跨仓库迁移

页签部件(`agent_tabs.rs`)、菜单弹层(`menu_overlay.rs`)和 `review_native` / `native_host` 的 wry 样板目前是 Digger 侧实现,后续应迁入 byteui / bytehost 共用。涉及两个仓库的发版与两边重钉,需要用户排期。

## 另有两条已知但不属本期三条的缺口

- 历史导入部分失败拿不到失败明细:需要 `TranscriptService` 返回带失败明细的结果,走 Dozer 新 libs tag(见 `notes/2026-10-26-conversations-s7b.md`)。
- 搜索框右键菜单没有宿主设施,`TextInputMenuOpen` 为空操作。
