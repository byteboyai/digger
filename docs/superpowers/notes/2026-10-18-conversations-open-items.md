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

## 已解决(续)

### 搜索框右键菜单(G1)—— 已完成(整框语义)

会话搜索框右键弹原生菜单:剪切、复制、粘贴、全选。**语义是整框的:** 剪切/复制作用于整个搜索词,粘贴追加到末尾,全选是精确的。原因:Digger 是标准 iced 应用,没有 Dozer 那套「合成 ⌘X/C/V/A 回灌给输入框」的自定义事件循环,而 iced 0.14 的输入框操作读不到选区。⌘C/V/X/A 快捷键仍是 iced 原生的精确选区语义。要做到按选区的菜单,需要在 byteui 的输入框上加「读选区」能力(新 byteui 版本、双方重钉)。仅 macOS;其它平台保持空操作。Project/Files/Todo 的输入框菜单未做。

## 仍然未决

### 3. 跨仓库迁移(X1)

**阶段 A 已完成(2026-10-28):** 页签部件、菜单弹层已迁入 byteui 0.5.0(Digger 的 `agent_tabs.rs`、`tab_chrome.rs`、`menu_overlay.rs` 缩成接线)。**阶段 B 未做:** `native_host.rs`(可信原生子 webview 执行器)及 `review_native` 等四个使用方、`markdown/host.rs` 迁入 `bytehost-desktop`,单独排期。计划:`dozer/docs/superpowers/plans/2026-10-28-x1-cross-repo-migration.md`。
