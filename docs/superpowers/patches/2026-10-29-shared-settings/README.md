# 共享设置复审修复补丁

本次会话只能写 Digger 与临时目录，bytehost、Dozer 原仓库为只读。因此这两个仓库的修复保存在可直接应用的补丁中，并已在临时源码副本中测试。Digger 端的修复直接写入本仓库。

Dozer 执行者可阅读[Dozer 侧实施计划](dozer-plan.md)。`dozer-plan.patch` 只新增该计划文档，不修改代码；在 Dozer 根目录执行以下命令，会创建 `docs/superpowers/plans/2026-10-29-shared-settings-review-fixes.md`：

```bash
git apply ../digger/docs/superpowers/patches/2026-10-29-shared-settings/dozer-plan.patch
```

在 Digger 根目录执行以下命令应用到现有的共享设置实现上：

```bash
git -C ../bytehost apply --check ../digger/docs/superpowers/patches/2026-10-29-shared-settings/bytehost.patch
git -C ../dozer apply --check ../digger/docs/superpowers/patches/2026-10-29-shared-settings/dozer.patch
git -C ../bytehost apply ../digger/docs/superpowers/patches/2026-10-29-shared-settings/bytehost.patch
git -C ../dozer apply ../digger/docs/superpowers/patches/2026-10-29-shared-settings/dozer.patch
```

补丁内容：

- bytehost：审批卡恢复 `plan_view.title`（应用名称与 ID）；应用管理视图增加当前 UI locale，标题按该语言解析；补充中英文与无投影时的回退测试。`AppsView::new` 默认英文，保留 Digger 当前语言行为。
- Dozer：将设置页当前 locale 传入共享视图。

这两个补丁不包含临时构建目录或本地测试用的 Cargo path 改动。Digger 使用 `AppsView::new`，可以在应用补丁前后编译。

## 版本依赖仍待发布

bytehost `v0.4.17` 不包含新增设置 crate；读取远端 main（`87f1c28815f1b64cdad67a2d8639c0760d2d6fb2`）也没有它们。不能用一个不存在的 tag / rev 代替当前声明。

应用补丁后，需要先将 bytehost 的完整共享设置实现提交并发布为可获取的 tag / rev，再统一更新 Dozer、Digger 的全部 bytehost pin；Dozer 的页内查找库快照也需一并发布并更新 Digger。随后在无本地 patch 的 checkout 中更新锁文件、运行构建及 `scripts/check-shared-deps.sh`。在这些步骤完成前，发行构建问题仍未解决，不能标记整体修复完成。

验证结果见[复审修复记录](../../reviews/2026-10-29-shared-settings-fixes.md)。
