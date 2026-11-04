# 统一设置第二轮问题：直接修复记录

> 后续状态（2026-11-04）：共享修复已从隔离克隆推送并发布为 bytehost `v0.4.23`、Dozer `libs-2026-11-04a`，详见[发布记录](2026-11-04-shared-settings-release.md)。以下保留修复当日的本地交付状态；原只读工作树未被覆盖，实机验收仍未完成。

- 日期：2026-11-03
- 对应：[第二轮审核](2026-11-03-unified-settings-dialog-review-round2.md)。
- 状态：Digger 修复已落到当前工作区；共享仓库修复已在可写副本完成、集成测试通过并生成补丁，**尚未写回只读的 bytehost / Dozer 原工作树**。发布和实机仍未完成。

## 改动与部署状态

| 审核项 | 修复 | 当前状态 |
|---|---|---|
| RR01 | resize 无 Output，改为 chain 串接绑定；正式测试执行真实 Created Task 流，断言 Resize 后产生原生 Run | Digger 已落地 |
| RR02 | 同产品 Admission 为每 provider 持有完整凭据 + 元数据提交锁；等待后重查准入，取消 / 新请求登记不等待 Keychain I/O | bytehost 补丁已验证 |
| RR03 | query 查询全部 provider；凭据已提交、元数据缺失返回 unreconciled 状态；共享页显示恢复说明，允许重新连接 / 断开，不假冒未连接 | bytehost / Dozer 补丁已验证 |
| RR04 | 超时将旧实例转为独立退休映射，Destroy 仍能定位真实窗口，发出实际 Close；Closed 只回收对应旧实例，不影响替代窗口；强制清理不再循环排超时 | Digger 已落地 |
| RR05 | Dozer 先经过共享 UI 派发，依据事件消费结果 / IME 组合状态判断 Esc；应用及高级页确认优先取消。Digger 同步映射应用确认的取消意图 | Digger 已落地；共享 / Dozer 部分补丁已验证 |
| RR06 | settings 使用 fallible native / GPU factory，真实创建错误转为 Failed；部分资源释放、父窗 Toast 说明错误，再次打开设置重试同会话 attempt；其他旧弹窗调用兼容 | bytehost / Dozer 补丁已验证 |
| RR07 | 不把本地未发布代码伪装成 v0.4.22；没有移动 tag 或填写不存在的版本 | 仍受阻：共享修复应用后需发布统一版本，再更新 pin / lock 与干净验证 |

Digger 的窗口槽位和退休实例分离，收到旧 Closed / timeout 不会关闭新会话；创建中取消也等待真实 Closed，而不再提前伪造销毁完成。没有修改用户原有 Cargo.lock 或个人 `.cargo/config.toml`。

共享补丁新增的查询字段属于 API 变化，所有已知 literal 和 UI 消费者已同步。Keychain service、provider key、账户 JSON schema 和两产品数据目录不变；不迁移或复制用户凭据。部分提交后的“重新连接”由用户明确输入并校验 Token；“断开”可删除已保存的凭据，即使没有用户名元数据也不会被禁用。

## 测试证据

[原始日志](artifacts/2026-11-03-unified-settings-dialog-fixes/digger-full.log)已保存。除直接 Digger 的两个 locked 回归外，集成测试均在可写副本中使用修复后的共享代码与本地开发依赖完成；这不等于远端 tag 验证。

| 范围 | 结果 |
|---|---|
| Digger 当前真实工作区，`--locked --offline` 新回归 | 2 通过；与原共享开发工作树兼容 |
| Digger 最终源码 + 修复后共享副本，全量串行 lib | 740 通过，45.17s |
| bytehost-git-accounts | 32 通过，含凭据 set 内暂停的旧 / 新提交及部分提交重查 |
| bytehost-settings | 49 通过，含重开后可恢复 / 可断开的部分提交 |
| bytehost-panel | 103 通过，含 Esc 取消内层确认 |
| bytehost-desktop `--all-features --lib` | 99 通过，含 native factory 失败不创建 renderer / GPU 失败释放部分窗口 |
| bytehost-settings-ui `--all-targets` | 7 通过，两个既有示例编译通过 |
| Dozer settings | 49 通过，含应用与高级确认的 Esc 优先级 |
| Dozer `cargo check -p dozer-app --all-targets` | 通过，保留既有 warnings |
| 对原 bytehost / Dozer 工作树 `git apply --check` | 均通过，未应用到原目录 |

原故障探针已改成检查正确行为的正式测试，没有把“复现故障的 PASS”当作修复通过。测试只用临时数据、fake 网络 / 凭据，没有登录真实账号或读取真实 Keychain。

## 共享改动交付与剩余边界

补丁及前后文件校验值：[共享修复补丁](../patches/2026-11-03-unified-settings-dialog-fixes/README.md)。可写实现副本在 `/private/tmp/digger-unified-fix/bytehost` 和 `/private/tmp/digger-unified-fix/dozer`，Digger 集成副本在同目录的 `digger`。

当前运行环境只允许修改 Digger / 临时目录；不能修改兄弟仓库原工作树。因此没有把共享副本已通过写成已部署。补丁检查不会覆盖现有未提交实现，应用前需在可写环境再次检查。

剩余步骤：

1. 在可写上下文应用 bytehost / Dozer 两份补丁，复核并提交完整的共享实现（含开发方原有未提交改动）。
2. 在正常 macOS 图形会话验证真实父子窗口、输入法、Esc、创建失败重试、wry 层级、多屏和销毁；本轮仅完成无窗口自动验证，没有声称实机通过。
3. 按发布授权完成共享版本与 Dozer 库快照收口，再更新所有消费者 pin / lock，执行无个人 path patch 的 locked 构建与门禁。旧 v0.4.22 缺 crate 的阻碍仍然存在。

本次没有推送或发布任何共享仓库，也没有把 spec / plan 整体标为验收完成。
