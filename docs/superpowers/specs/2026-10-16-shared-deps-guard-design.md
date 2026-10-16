# 共享库依赖守卫(shared-deps-guard)设计

日期:2026-10-16
状态:待审阅

## 1. 背景与目标

Digger、Dozer 都消费 `github.com/byteboyai/*` 下的共享库(bytehost、byteui、bytegit,以及 Dozer 抽出的 `dozer-model`、`dozer-services`、`dozer-store-project`、`dozer-panel-*`)。Dozer 的 agent 探针发现 Digger 的共享库版本落后,同时暴露了几类已经发生过的问题:

- Digger 固定在 bytehost `v0.2.0` / byteui `v0.4.1`,远端已有 bytehost `v0.4.0` / byteui `v0.4.4`,没有任何机制提醒落后。
- 开发期的本地 `[patch]` 指向兄弟 worktree,worktree 被删后 `cargo build --workspace` 直接失败。
- Dozer 的 `dozer-store-project` 只在本地 `main`(领先远端 25 个提交),Digger 无法改成纯 git 依赖。

**目标:** 消费者能安全地消费共享库的某个版本:落后能被发现、固定是显式且有期限的、不会把本地路径或未推送的依赖带进 `main`。

**本期范围:** 消费侧。贡献侧(改共享库前验证消费者能否构建、语义化版本约定)不在本期。

**不在本期做:** 升级 Digger 的 bytehost/byteui 版本;处理 Dozer 未推送的提交;自动升级机器人;CI。这些是脚本跑出结果之后的独立任务。

## 2. 总体方案

一个只用 Python 标准库的检查脚本,加一份记录"刻意固定"的清单,由 `pre-push` 钩子触发。commit 时不检查(提交太频繁)。

**脚本归属:** 正本放在 bytehost 仓库(Digger 与 Dozer 共同的最底层依赖),随 bytehost 打 tag。每个消费者把单文件 vendor 到自己的 `scripts/`,文件头记录来源 tag。脚本可报告"我的正本有新版"。

## 3. 清单与版本来源

**版本只在各 `Cargo.toml` 里写一份,清单不重复它。** 脚本扫描 workspace 内**所有** `Cargo.toml`(Dozer 没有统一的 `[workspace.dependencies]`,各 crate 各自声明),取出指向 `github.com/byteboyai/*` 的 git 依赖,按仓库分组。同一仓库下的多个 crate 视为同一个共享库。

仓库根的 `shared-deps.toml` 只记录 `Cargo.toml` 表达不了的意图:

```toml
# 刻意固定在某个版本的共享库;没写在这里的默认应该跟上最新 tag。
[pin.dozer]
reason = "等一次性同步所有共享面板(需要 Digger 有 locale 来源、bytehost i18n 已发布)"
review_by = "2026-11-15"
```

- 有 `pin` 的库不报"落后";`review_by` 过期后提醒,不失败。
- 清单里有而 `Cargo.toml` 里已不存在的条目,报"清单过期"。

## 4. 检查规则

| 规则 | 级别 | 含义 |
|---|---|---|
| `local-patch` | block | 入库的 `Cargo.toml` 中存在指向本地路径的 `[patch]` |
| `lock-not-git` | block | `Cargo.lock` 中共享库条目不是来自远端 git(被本地路径覆盖) |
| `ref-missing` | block | 固定的 tag/rev 在远端明确不存在 |
| `ref-mismatch` | block | 同一共享库下多个 crate 的 tag/rev 不一致 |
| `behind` | warn | 无 `pin` 的库落后于远端最新 tag(按语义化版本比较;rev 固定的只查存在性) |
| `pin-expired` | warn | `pin` 的 `review_by` 已过 |
| `manifest-stale` | warn | 清单条目在 `Cargo.toml` 中已不存在 |
| `ref-unverified` | warn | 联网超时等原因无法验证,放行 |

只检查 `byteboyai/*` 的 git 依赖,不管 crates.io。

## 5. 触发方式

- **手动:** `scripts/check-shared-deps.sh`,输出完整报告,不阻断任何操作。
- **pre-push(推 `main` 或 tag):** `block` 级问题阻断推送。
- **pre-push(推其他分支):** 同样的检查,`block` 降级为警告。
- 每次 pre-push 末尾顺带打印落后报告。
- 联网失败(超时等)按 `ref-unverified` 警告放行;只有远端明确没有该 ref 才产生 `ref-missing`。
- 钩子放在仓库内 `.githooks/pre-push`,每个克隆需执行一次 `git config core.hooksPath .githooks`,README 与脚本报错提示中说明。

规则本身不关心分支;"阻断还是降级为警告"由钩子根据推送目标决定。

## 6. 本地联调

开发期需要覆盖共享库时,把 `[patch]` 写进**不入库**的 `.cargo/config.toml`(两个仓库的 `.gitignore` 已忽略它)。提供 `scripts/dev-patch.example.toml` 作模板。仍允许直接改 `Cargo.toml`,但推 `main` 前会被 `local-patch` 拦下。

注意:无论 `[patch]` 写在哪里,构建都会把被覆盖的共享库在 `Cargo.lock` 中记成本地路径。因此 `lock-not-git` 是必需检查,它扫描 `Cargo.lock`。

## 7. 脚本结构

```
scripts/check_shared_deps.py       # 单文件,仅标准库(tomllib)
scripts/check-shared-deps.sh       # 薄包装
scripts/test_check_shared_deps.py  # unittest,fixture,不联网
scripts/dev-patch.example.toml     # 联调模板
.githooks/pre-push                 # 调用脚本并决定阻断/降级
shared-deps.toml                   # 固定理由
```

内部三层:

1. **解析:** 读所有 `Cargo.toml`、`Cargo.lock`、`shared-deps.toml`,得到按仓库分组的 ref 与 source。
2. **检查:** 每条规则一个纯函数,输入解析结果与"远端查询结果",输出问题列表。联网查询(`git ls-remote`)隔离成可替换函数。
3. **输出:** 每个问题带级别、规则名、一句话说明和修复提示。有 `block` 时退出码 1,否则 0。

输出示例:

```
BLOCK  lock-not-git   bytehost-core 在 Cargo.lock 里是本地路径,先 cargo update -p bytehost-core 还原
BLOCK  ref-missing    dozer rev 67ab0e6 在远端不存在,先推送再依赖
WARN   behind         bytehost v0.2.0 -> 最新 v0.4.0(落后 2 个 minor)
WARN   pin-expired    dozer 的 pin 已过 review_by(2026-11-15)
```

## 8. Dozer 的特殊性

- Dozer 既是消费者也是提供者。作为消费者,与 Digger 用同一份脚本(vendor 自 bytehost),扫描其分散的 per-crate 依赖。
- 作为提供者,Digger 对 `dozer-*` 的 rev 依赖由 Digger 侧的 `ref-missing` 兜住。Dozer 侧的 pre-push 额外打印"本地 `main` 领先远端 N 个提交"作为提示,不新增阻断规则。
- Dozer 的工作区含他人未提交改动,对 Dozer 的任何改动必须在独立 worktree/分支里进行,不动其主工作区。

## 9. 测试

- 每条规则至少一个"通过"和一个"失败"的单元测试。
- 解析层用真实 `Cargo.toml`/`Cargo.lock` 副本做 fixture,Digger 与 Dozer 各一份(覆盖 workspace 依赖与 per-crate 依赖两种形态)。
- 联网查询不进单元测试;实现后在真实仓库手动跑一次验证。

## 10. 落地顺序

1. 在 bytehost 仓库实现脚本、测试、模板、钩子,打 tag。(涉及推送 bytehost,届时单独征得同意。)
2. Digger vendor 脚本,加 `shared-deps.toml`(含 dozer 的 pin)、`.githooks/`、README 说明。
3. Dozer 在独立 worktree 里 vendor 同一份脚本与钩子。
4. 两个仓库各跑一次完整报告,把结果作为后续升级任务的输入。

## 11. 已知限制

- 仅覆盖消费侧,不验证共享库的改动是否破坏消费者。
- rev 固定的依赖没有可比较的版本号,只能检查其存在,无法报告落后。
- 钩子需要每个克隆手动启用一次。
- vendor 的副本只能提示正本有新版,不会自动更新。

## 12. 实现阶段修订(2026-10-16)

实现前拿真实仓库验证,发现需要澄清四点。它们细化第 3、4、5 节,不改变整体方案。

1. **按工作区分组(细化第 3、4 节)。** 每个 `Cargo.toml` 归属向上最近一个声明了 `[workspace]` 的目录(cargo 的规则);向上没有工作区的包自成一组。`ref-mismatch`、`lock-not-git`、`lock-duplicate` 都只在同一工作区内部判断,因为只有同一份 `Cargo.lock` 里的版本冲突才会拉进两份同名 crate。Dozer 的 `spike/v2-excalidraw` 自带 `[workspace]` 且用 bytehost `v0.2.0`,不应和主工作区冲突。
2. **新增规则 `lock-duplicate`(block,细化第 4 节)。** 同一个共享 crate 在 `Cargo.lock` 里有多个 git 来源。典型成因是传递依赖:Digger 用 bytehost `v0.4.1`,而它依赖的 `dozer-panel-*` 自己用 `v0.4.0`,于是解析出两份 `bytehost-core`,`PanelId` 之类类型互不兼容、byteui 的全局主题状态分裂。只看 `Cargo.toml` 发现不了。Dozer 的 `scripts/check-shared-pins.sh`(提交 `0ad8a334`)是它的早期、只覆盖 `crates/*/Cargo.toml` 的版本,新脚本落地后取代它。
3. **pre-push 检查被推送的提交,不是工作区(细化第 5 节)。** 开发时 `.cargo/config.toml` 里的本地 `[patch]` 会让工作区的 `Cargo.lock` 带本地路径,这是正常的开发状态。脚本新增 `--rev <提交>`:用 `git archive` 取出该提交里的 `Cargo.toml` / `Cargo.lock` / `shared-deps.toml` 再检查。钩子对每个被推送的引用分别检查(删除引用跳过)。
4. **补充规则与限制(细化第 4 节、第 11 节)。** 新增两条 `info` 级提示,不影响退出码:`ahead-of-remote`(本地 `main` 领先 `origin/main` N 个提交,用本地引用,不联网)与 `guard-source`(bytehost 有比 vendor 来源更新的语义化 tag,正本可能有更新)。非语义化 tag(如 `libs-2026-10-16`)只检查存在性,不比较落后。rev 固定的依赖先按"是否为某个远端引用的末端"判断,否则用 `git fetch --depth=1 --filter=tree:0 <完整 sha>` 兜底;短 rev 无法确认,只警告。
