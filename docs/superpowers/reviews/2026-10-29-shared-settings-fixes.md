# 共享设置复审修复记录

日期：2026-10-29。对应上轮代码审核的五项 finding。

## 已写入 Digger

1. 从设置直达应用页、或从主题页切到应用页时，都调用现有 `AppsState::Opened`。重复打开复用正在启动或已就绪的服务，不创建第二份安装状态。新增真实临时服务启动测试，覆盖两个入口和重复激活。
2. 主题变化时，Files / Project 的所有已打开预览页签调用共享库的主题重载；包括未选中页签。新增两个面板、三个文件页签的回归测试。
3. 用量与待办按当前主题创建网页；切换主题时丢弃旧 ready / 去重状态、推进代数并重新握手，重新推送数据。群聊主题纳入文档身份，旧页面不能消费新页面的数据。新增相关状态测试。
4. Markdown 编辑器首次打开读取当前主题；切换时只改 CSS，不销毁文档、光标或撤销栈。隐藏的编辑器也更新。网页包已重建，harness 增加 T24：切换明暗后文档 / revision 不变，原编辑仍可撤销。
5. 会话审阅页通过 byteui token 更新共享 CSS 变量，原地切换样式并保留加载身份与滚动位置；新建文档也注入当前样式。
6. 关闭后的迟到设置窗口创建回包不再重新登记已关闭句柄。

## 已准备的跨仓库补丁

[补丁及应用说明](../patches/2026-10-29-shared-settings/README.md)。两个补丁均通过原工作区的 `git apply --check`，但受本次会话写入范围限制，未应用到原 bytehost / Dozer。

- 审批卡恢复应用名称与 ID。
- 标题解析使用注入的当前 UI locale，Dozer 转交实际语言；中英文及 fallback 有回归测试。

## 验证

- Digger desktop **715 项**、app-host **39 项**、paths **12 项**测试串行回归通过；新增设置初始化、主题握手及预览页签回归。此轮使用修复后的共享 UI 副本联调。并行执行曾有一项未改动的 Agent hook 测试因安装子进程超时失败，串行重跑恢复通过。
- 修复副本中的 bytehost-settings **22 项**、bytehost-settings-ui **8 项**测试通过。
- 修复副本中的 Dozer `cargo test -p dozer-app settings --offline`：**47 项通过**。
- 编辑器网页包构建通过，包括 TypeScript 检查及 12 个生产 parser fixtures 对照；已更新 `web/dist/cm.html`。
- 网页包 `--check` 一致性验证、host-deps 依赖边界门禁与 `git diff --check` 通过。
- 当前本地开发配置下的 `cargo check -p digger-desktop --offline` 通过，确认 Digger 在两个跨仓库补丁尚未应用时仍可编译；这不等于无 patch 的发行构建通过。
- 已尝试真实窗口 lifecycle 自检，macOS 图形服务返回连接无效，未出现 SUMMARY，结果为失败。此环境下无法确认窗口、主题实际绘制及 T24 harness；不能将它们记为实机验收通过。

## 尚未解决

发行依赖仍指向不包含新增设置 crate 的旧 bytehost tag。本次核查远端亦无可用版本，不能伪造新 pin。需要在可写的 bytehost / Dozer 工作区应用补丁并完成共享库发布，再统一 pin、重建无 patch 的锁文件与验证干净消费者。故五项问题中，Digger 功能问题已落地，共享 UI 修复以补丁交付，依赖发布问题仍未完成。
