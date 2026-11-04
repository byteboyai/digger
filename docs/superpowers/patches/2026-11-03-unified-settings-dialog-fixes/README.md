# 共享仓库修复增量补丁

> 2026-11-04：修复已包含在远端 bytehost `v0.4.23` / Dozer `libs-2026-11-04a`，见[发布记录](../../reviews/2026-11-04-shared-settings-release.md)。这些补丁保留为历史证据；新 tag 已包含修复，不要再次套用。

这些补丁叠加在 2026-11-03 第二轮审核时的**现有未提交统一设置实现**上，不适用于直接从 v0.4.22 创建的干净工作树。`source-manifest.json` 保存每个目标文件修改前后的 SHA256。

当前会话仅能写 Digger；bytehost / Dozer 原工作树只读。因此修复已在 `/private/tmp/digger-unified-fix/{bytehost,dozer}` 完成并测试，但没有写回原仓库。两份补丁已分别对原工作树执行 `git apply --check`，均通过。

- `bytehost.patch`：provider 提交串行化、部分提交查询 / 恢复 UI、可失败的建窗和 GPU 初始化、事件消费结果与 IME 跟踪、应用确认的 Esc 语义及正式回归测试。
- `dozer.patch`：新查询 DTO 接线、设置创建错误收尾与可见重试、先派发再处理未消费的 Esc、确认优先级及回归测试。

在能够写两个仓库的正常终端中，从 Digger 根目录执行（先检查两份，再应用）：

```bash
set -e
digger_root="$PWD"
patch_root="$digger_root/docs/superpowers/patches/2026-11-03-unified-settings-dialog-fixes"
git -C ../bytehost apply --check "$patch_root/bytehost.patch"
git -C ../dozer apply --check "$patch_root/dozer.patch"
git -C ../bytehost apply "$patch_root/bytehost.patch"
git -C ../dozer apply "$patch_root/dozer.patch"
```

若检查失败，先核对已有改动；不要强行覆盖文件。这两份补丁需要一起消费，新 `AccountSnapshot.unreconciled_providers` 已同步全部已知消费者与测试。Digger 当前修改可独立通过新增运行时回归；完整账户 / Dozer 修复需应用共享补丁。

验证命令和实际证据见 [修复记录](../../reviews/2026-11-03-unified-settings-dialog-fixes.md)。本地 patch 测试不等于已发布依赖可用；应用补丁后仍需完成统一版本发布、无 patch locked 消费和实机验收。这里没有伪造新 tag，也没有修改个人 Cargo 配置或凭据格式。
