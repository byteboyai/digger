# 群聊面板遗留缺口

日期:2026-10-27 起草,2026-10-28 收尾更新。收尾计划:`dozer/docs/superpowers/plans/2026-10-27-s7-s8-closeout.md`。

## 已解决

1. **新建群失败时草稿丢失 —— 已修(Dozer `d6169951`,libs `libs-2026-10-28a`)。** 失败时恢复弹窗与主题(仅当主题框为空,不覆盖用户之后的新输入),错误进 outbox。
2. **列表收起的展开按钮条 —— 已对齐 Dozer(Digger `b2ee423`)。** 收起/展开按钮、展开条与顶部预留条一并删除。**已处理(R1):** 这两个按钮是群聊收起的唯一入口,而布局文件会持久化每个面板的 `collapsed`;读盘时把群聊的「已收起」按展开处理(`shell::group_chat_never_collapsed`,保留分栏比例,只改读入文本不写盘),不会有用户被锁在收起态。
3. **`list_collapse_button` 空占位 —— 已删(Dozer `11628d12`)。** 该方法从未被库调用;Dozer 群聊本身没有收起 UI。
5. **`no_todo_text_anywhere_in_the_view`** —— S8c 已处理,不变。
6. **三个不能发言的成员种类 —— 部分解决(Qodercn、Codebuddy 已可发言,Opencode 仍不行)。** Qodercn 已支持群聊只读发言(Dozer `dc9c9c06`,以 `--tools Read,Grep,Glob` + 禁用清单 + `--no-session-persistence`,经真实 runner 冒烟验证),Codebuddy 登录后补测通过(Dozer `53a7100a`,libs `libs-2026-10-28b`)。新建群弹窗说明现为「目前仅 Claude、Codex、Qodercn、Codebuddy 可以发言」。调研记录:`dozer/docs/superpowers/specs/2026-10-27-group-chat-agents-readonly-research.md`。
   - **Opencode 仍不支持:** 无限制工具参数;配置里写 `webfetch: deny` 后联网仍成功;无「不落会话」开关;提示词只能走 argv。
   - Opencode 仍可加入群、发言时失败;是否在弹窗里隐藏属产品决定。

## 仍然未决

4. **跨仓库迁移(X1)。** 页签部件、菜单弹层、`native_host`/wry 样板迁入 byteui/bytehost 共用,单独排期。
