# 写字台未完成项续作与验证（2026-11-06）

状态：**W1 仍为部分实施**。本轮补齐后台 IO、交互入口、镜像恢复、故障回归和备份；真实 GUI/IME、原生生命周期、IPC/paint 性能与最终远端 CI 尚未通过。W2–W5 未开始实现。

基线 HEAD：`8788b567e34b3cbab77923ee5a54d8f0c358e4e8`。交付内容在工作区，已有修改和个人 Cargo.lock 保留，原工作区未创建提交或推送；另在隔离副本准备本地 CI 候选提交，仍未推送。源码身份见 artifacts 的 `source-sha256.json`，不能将 HEAD 当作这些未提交实现的版本标识。

## 已补齐的实现

- 文稿 IO 使用容量 8 的串行后台队列；新建、导入、载入、保存、恢复删除及镜像处理不在 UI 线程进行磁盘操作。排队立即拒绝过载；SQLite/写锁等待上限 5s。载入可停止等待，已准入操作继续结算，迟到结果不会改变当前会话。
- 页面 flush 后冻结输入，显示“保存后继续／保留恢复草稿后继续／取消”。失败留在原文稿；取消后旧持久化回包不能越过下一次 flush。项目切换/退出从 ProductShell 真实消息经过同一屏障，隐藏面板时会显示选择入口。
- 文件树、Files/Project 预览页签的 Markdown 右键菜单增加“在写字台编辑”，普通打开/双击仍走预览；消息带项目 ID。文稿管理支持持续新建/导入及完整文稿列表。
- 接受恢复草稿时显式恢复待提交 history；普通 durable 载入保持不恢复已提交 history。非空 aliases/history 的恢复→保存→镜像→重开逐字段回归证明二者边界，已准入保存在面板及其 future 被丢弃后仍结算。
- 自动恢复为 500ms 静默、连续输入最多 2s 的合并策略；纯时钟测试无真实 sleep。恢复正文可以与已保存正文预览；丢弃在后台执行，损坏/未来格式/身份不匹配保留原件并显示问题。
- 启动和空闲维护重放 Outbox，每批 8 条、空闲间隔至少 5s。失败最多自动尝试 8 次，明确重试可重新启动；外部冲突持续挡住同文稿后续任务。跨进程 worker 锁、单调镜像 revision、原子 Outbox ACK、防止 rename 后未 ACK 的重复写入均已接齐。
- 外部文件有全文比较、导入预览候选为独立副本、明确确认替换。替换重新核对 durable revision 和候选字节摘要；确认过期时拒绝覆盖，不自动合并。损坏草稿原件保留，官方正文仍可编辑保存；硬链接路径不能覆盖原件。恢复/镜像后台回包有独立操作编号，旧结果不会解除新任务的忙碌状态。
- 文稿写事务改用 BEGIN IMMEDIATE、FULL 同步；数据库文件身份/store UUID 变化后拒绝旧连接继续读写和镜像。恢复封装大小有界，严格验证块状态；数据库、草稿与镜像目录的链接被拒绝。禁止运行中单独替换主库并留下旧 WAL，详见恢复操作文档。
- 提供 `writing_backup` 实用入口：一致 SQLite 快照、权威 Markdown、实际外部文件、working 草稿和 manifest 摘要；新目录恢复、更换实例 UUID、草稿重绑定，源项目与备份保持。W1 不含尚未托管的图片/资源或共享项目注册表。

## 自动验证

验证环境：macOS arm64；正式依赖副本移除个人 `.cargo` patch，保留独立正式锁，命令均带 `--locked --offline`；独立 target 目录。原工作区个人 patch/锁未替换。

| 项目 | 结果 | 证据 |
|---|---|---|
| desktop lib，全量串行 | 795 passed | `desktop.log` |
| store lib，全量串行 | 92 passed（`store-final.log`） | 包含 Writing 子进程场景；子进程自己的小计不重复累计 |
| editor lib，全量 | 47 passed，含 book 规模回归 | `editor.log` |
| TypeScript + Node 同步 | 类型检查通过，34 passed（`typescript.log` / `node.log`） | 本轮执行 `npm run check && npm test`；协议/TS 本轮无新增修改 |
| 正式 workspace all-targets | 通过 | `workspace-check.log` |
| parser / 嵌入 bundle | 12 fixtures，bundle 一致 | `web-bundle.log` |
| 宿主依赖门禁 | 通过，含 self-test 15 项 | `host-gate.log` |
| 正式共享来源门禁 | 0 阻断、0 警告 | `formal-shared-gate.log` |
| 原工作区共享来源门禁 | 仍因个人 patch/锁阻断 | `personal-shared-gate.log`；未把隔离结果冒充原锁通过 |
| 自检入口故障回归 | 4 passed | `selftest-entry-tests.log` |
| fmt / 源码及说明文档 whitespace | 通过 | 原始日志/嵌套补丁归档单列，不作为源码空白门禁 |

新增故障证据包含：真实子进程在 save commit 前/后退出，回复丢失后幂等重放，镜像 rename 后未 ACK 重放，草稿 rename 前退出，双进程同 revision 恰好一个成功，SQLITE_FULL/只读事务不推进任何事实，真实目录写权限失败保留源文件，超限外部替换保留原件，数据库实例替换后拒写，外部冲突跨批保持，条件确认过期拒绝，备份摘要损坏拒绝恢复。有界队列拥塞和 worker panic 后继续工作也有回归。

## 存储规模样本

每种规模 5 次 release 样本，生产 Doc/Store/Recovery API，临时真实工程。下表为中位数，单位 ms；原始逐次数据见 `writing-bench.jsonl`。这是后端样本，不代表 GUI 延迟或同机独占性能基线；未测队列等待、真实 IPC、中文输入法或 paint，不以这些数字关闭 T6。

| 块数 | Doc edit | 保存 | 镜像 | 草稿写入 | 草稿读取 | 重开/加载 |
|---|---|---|---|---|---|---|
| 100 | 0.032 | 0.453 | 13.400 | 8.621 | 0.357 | 0.862 |
| 1000 | 0.074 | 1.143 | 14.294 | 9.340 | 0.956 | 1.308 |
| 5000 | 0.156 | 2.894 | 18.684 | 11.735 | 2.689 | 2.576 |

追加后端队列样本：`writing_queue_performance_samples` 显式手动执行通过（1 个 ignored-by-default 性能入口），容量 8 的生产队列中先提交真实草稿写入，再提交 SavePlan；每规模 5 次 debug 样本。

| 块数 | 队列等待中位 ms | SavePlan 执行中位 ms |
|---|---|---|
| 100 | 9.109 | 15.086 |
| 1000 | 13.398 | 26.913 |
| 5000 | 31.388 | 81.888 |

原始数据见 `queue-bench.jsonl/log`；debug 队列样本与上方 release 直调样本不能当作同一性能指标比较。另运行既有 `bench-editor-sync.sh`，对照 `88fcf4e` 的 release Rust Bridge+JSON 门禁通过：100/1000/5000 块 p95 分别加速 2.46×/3.95×/4.51×；5000 块回复体积减少 99.9074%。Node apply 样本另列 `frontend.jsonl`，同样不代表真实 webview/paint。工具链仍报告缺少 libLLVM.dylib 导致 rust-objcopy 去调试信息失败的警告，命令最终成功，未隐去警告。

## GUI 与 CI 的实际边界

当前 computer-use 的 Accessibility 和 Screen Recording **均为 granted**（`gui-permissions.json`），已安装 Digger 可以恢复窗口并读取。因此不能继续沿用“权限被拒绝”的旧结论。

本轮构建的二进制在隔离 DIGGER_DATA_DIR 下启动 harness，30 秒后仍无 SUMMARY，记录 `com.apple.hiservices-xpcservice` connection invalid / LaunchServices 错误，按失败/未验处理（`gui-final-harness.log/json` 提供该样本 executable SHA-256）。另外临时 .app 启动返回 kLSNoExecutableErr。没有把旧安装窗口当作新源码通过证据，也未断言这些现象已经证明与修改无关。后续源代码变化仍须在可成功启动的图形运行环境重新验收。

Consumer workflow 已扩大为全部 `markdown::` 回归及 `writing::` 故障/备份用例；slow workflow 加入 release 文稿故障/规模样本、单列 debug 队列样本，并准备 Node 同步基线。没有推送、没有最终 CI URL；本次提供本地正式组合证据及正式锁候选，不能用其它 SHA 的绿灯替代。原工作区 `.git` 只读，个人锁保持原状。CI 候选在临时副本分支 `review/writing-workbench-20261106` 准备，使用已通过检查的正式锁；候选信息单列 `ci-candidate.json`，推送仍需单独授权。

## A1–A12 与剩余工作

| 验收 | 当前结论 |
|---|---|
| A1 入口 | 文件树/两类预览/文稿管理已接线，ProductShell 文件消息回归通过；实机交互待验 |
| A2/A3 事务与身份 | 自动证据通过；正文/块身份/CAS/幂等/原子性/崩溃/跨进程竞争 |
| A4 flush/水位 | 协议、IME 模拟和保存中输入回归通过；真实中文 IME 待验 |
| A5 生命周期 | 状态及真实项目/文件/退出消息通过；原生窗口/设置遮罩/焦点等实机组合待验 |
| A6–A8 恢复/镜像/外部候选 | 域和状态测试通过；完整 GUI 操作仍待验 |
| A9 实际编辑体验 | 未通过实机验收 |
| A10 正式组合/CI | 正式组合本地通过；原个人锁阻断保留，最终远端 CI 未执行 |
| A11 规模 | 后端 100/1000/5000 块样本完成；真实 IPC/paint 未完成；后端队列样本和既有纯 Bridge/Node 基线已补充 |
| A12 备份与范围 | W1 备份→恢复→重开及回退操作文档完成 |

继续执行的验收工作：在可启动最终二进制的图形会话完成中文组字、保存中继续输入、undo/redo、主题/焦点/遮罩、切换、关闭重开及 lifecycle；采集真实 IPC/paint 性能并结合队列分段样本；形成最终提交/正式锁组合后取得对应远端 CI。T5 的无窗口领域/消息组合矩阵已补齐；T4 的无窗口生命周期状态回归完成，设置遮罩层级和实际显示/输入体验仍按 T4/T6 未验保留，不把状态回归冒充 GUI 证据。


## T4/T5 矩阵补验

在原有 50 项写字台回归基础上新增 7 项，定向结果为 **57 passed、1 ignored**（性能入口已另行手动通过），见 `t5-gap-matrix.log`：

| 场景 | 验证结果 |
|---|---|
| IO 队列受控阻塞时载入 B | A 的旧页面输入在载入中和 B 生效后均不能修改新会话 |
| 慢载入取消后发起下一次载入 | 旧成功/失败回包不能结束或覆盖新会话 |
| 保存已准入时完整替换 SQLite 文件集 | 旧库拒写；旧保存失败回包不改变同路径新 store 的正文、身份和错误状态 |
| 隐藏/主题同步/创建 Retry | 脏缓冲和文稿身份保留，旧创建失败不能污染新 generation |
| SQLite trigger 拒绝事务写入 | 未推进 durable revision，仍能输入；撤去故障后重试成功 |
| 切换到摘要损坏的目标文稿 | 原脏缓冲保留并仍能保存；损坏目标不被自动修复或覆盖 |
| 外部文件在预览后再次改变 | 过期确认拒绝，未保存输入及外部原件保留；重新预览后可另存独立副本 |

这些是生产域/适配器的状态和消息证据，没有实际执行 NSView 创建/主题/设置窗口层级。领域只读/SQLITE_FULL 测试与本次 UI 写入拒绝回归分列，不声称后者使用了只读文件系统。

GUI 的最新补充尝试中，另一原生窗口工具明确拒绝 Finder/Digger 访问（`native-access-block.json`）；这与 Orca 显示 macOS 权限 granted 是不同层的授权。已请求本次 Digger/隔离 QA 窗口访问授权，未收到授权前不继续该工具的 GUI 操作。
