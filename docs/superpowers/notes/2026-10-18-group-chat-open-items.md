# 群聊面板遗留缺口(待用户裁决)

日期:2026-10-27。

1. **新建群失败时草稿丢失。** 库的 `NewGroupSubmit` 先关弹窗并清空主题框,再异步创建;失败时宿主只能在通知条报错,无法恢复弹窗和草稿。要修需要改 Dozer 的 `dozer-panel-group-chat`(新 libs tag,Digger 重钉)。
2. **列表收起的展开按钮条。** Digger 在聊天区顶部为 iced 展开按钮让出一条;Dozer 没有。是接受,还是改成与 Dozer 一致(例如把展开入口放进页面或原生菜单),待定。
3. **库适配里的 `list_collapse_button` 是空占位。** 该 trait 方法只能返回库的 `Message`,Digger 的折叠按钮在产品层另加。要消除需要库放宽宿主消息类型。
4. **跨仓库迁移。** 页签部件、菜单弹层、`native_host`/wry 样板迁入 byteui/bytehost 共用。
5. **`no_todo_text_anywhere_in_the_view`** 测试原先没有任何断言;S8c 已删除并以 `the_failure_shell_names_the_reason_has_no_todo_text_and_retry_clears_it` 取代(聊天页现在有转待办入口,「视图里不得有待办文字」的前提已不成立)。
6. **三个不能发言的成员种类**(Codebuddy、Opencode、Qodercn)仍可加入群;新建群弹窗的常驻说明已注明「目前仅 Claude、Codex 可以发言」。是否隐藏属产品决定。
