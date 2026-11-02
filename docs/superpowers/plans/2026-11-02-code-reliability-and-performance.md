# Digger 代码恢复性与性能优化 Implementation Plan

- 日期：2026-11-02
- 状态：R01–R08 复审修复已落地；整体为部分实施，A7 / 实机 / CI / 无 patch 项仍待验证（见[验收报告](../reviews/2026-11-02-code-optimization-acceptance.md)）
- Spec：[恢复性、性能与工程质量设计](../specs/2026-11-02-code-reliability-and-performance-design.md)
- 目标：先完成五类恢复性修复，再优化编辑同步和用量存储，最后拆分组合根并落实 CI。
- 技术栈：Rust 2024、rusqlite、Tokio、iced / wry、CodeMirror 6、TypeScript、Node 测试运行器。

## 执行约束与顺序

- 所有路径相对于 Digger 根目录。实施前读取当时适用的仓库指令，并重新确认源码、依赖和工作区状态。
- 不覆盖已有修改；编写本文时 `Cargo.lock` 已 dirty。个人 `.cargo/config.toml` 和全局 Cargo 配置不修改，不提交本地路径 patch。
- 以下是待执行任务，不因编写计划而开始代码修改、用户数据迁移或发布。
- 每项任务完成相应测试与产物更新后，形成可独立审阅的提交 / 变更。无需为纯文件移动添加实现镜像测试。
- 测试使用临时产品数据根；故障通过注入存储 / 时钟 / 传输接口复现，避免依赖 chmod 在特定用户下的偶然表现。
- Cargo 的 target 目录若不在可写范围，将 `CARGO_TARGET_DIR` 指向可写临时目录。不能把沙箱环境失败改成测试成功。
- 阶段 R：Task 0 → 1 → 2 → 3 → 4 → 5。阶段 P：Task 6 → 7 → 8 → 9。阶段 E：Task 10 → 11 → 12。
- P / E 可各自形成后续交付；本计划不要求多代理执行。验收矩阵必须分别标记已通过、失败、未执行和环境受限。

## Task 0：固定基线、复现与测量入口

**新增：** `docs/superpowers/notes/2026-11-02-code-optimization-baseline.md`。

- [ ] 记录 HEAD、dirty 文件、Rust / Node / macOS 版本、目标架构、Cargo patch、lock 实际来源和可用窗口 / 进程查询能力。
- [ ] 将 spec F1–F7 的复现转成可长期运行的测试场景清单；区分已复现故障、静态路径结论和性能假设。
- [ ] 运行 editor、store、app-host、paths 相关基线；完整慢速属性测试单独记录退出状态，补齐前轮未结束项。
- [ ] 记录 `npm run check`、fixture 和嵌入 bundle 当前一致性。环境失败保留错误输出，不修改预期掩盖问题。
- [ ] 固定 100 / 1000 / 5000 块语料及种子；测 Doc、Bridge + 序列化，保存逐样本数据，明确 Debug 探针与 Release 基线区别。
- [ ] 核对 Project 迁移的全部调用点、清理故障展示入口、usage 服务消费者和退出流程；记录将受影响的签名与所有权。

**验证：** `cargo test --locked -p digger-editor -p digger-store -p digger-app-host -p digger-paths`；web 目录执行 `npm ci`、`npm run check`、`npm run fixtures:check`；根目录执行 `bash scripts/build-editor-web.sh --check`。慢速 / GUI 测试按实际能力分开运行。

**完成条件：** 基线和可复现命令齐全，所有失败有分类；不要求已有缺陷在基线阶段通过。

## Task 1：项目迁移提交标记与重启恢复

**修改：** `crates/store/src/{migrate,projects,lib}.rs`、`crates/desktop/src/projects/` 及实际启动错误展示调用点。

- [ ] 先补“提交成功但 rename 失败”“提交后进程中止”“归档文件已存在”“两个迁移调用竞争”的回归场景。
- [ ] 在目标库增加 Digger 命名空间迁移记录；source 行复制和完成标记在同一事务提交，事务内重新检查标记。
- [ ] 返回结构化结果，区分未迁移、已迁移、归档待完成、真实冲突。归档待完成不阻止项目服务启动。
- [ ] 归档禁止覆盖已有目标；记录可重试状态，启动时只重试归档，不再次 INSERT。
- [ ] 为旧版“已复制、无标记”实现严格行级核对：projects、memories、history 的规范化来源投影必须全部一致；核对成功后事务补标记。
- [ ] 保留旧 schema 适配、ID 保留和真实冲突整体失败；修改只凭 exists 删除目标库 / sidecar 的失败清理逻辑，确保文件归属可证明。
- [ ] 保持共享项目 store 的 schema 所有权，不更改其 user_version，不使用 IGNORE / REPLACE 掩盖冲突。

**验证：** `cargo test --locked -p digger-store migrate`；相关 `open_in`、项目启动适配测试。使用子进程 failpoint 验证真实提交后中止和重启。

**完成条件：** A1；数据事实与归档状态分离，重复启动不再因自身已复制数据失败。

## Task 2：块统计溢出与匹配预算

**修改：** `crates/editor/src/blocks/reconcile.rs`、`crates/editor/tests/properties.rs`。

- [ ] 补英文重复词、中文重复片段、重复句子和 65535 / 65536 边界测试。
- [ ] 将频次及相关聚合统一为 usize；检查候选数量乘法与工作量计算，避免新的溢出入口。
- [ ] 测量长块字符 diff，冻结长度 / 配对预算与保守降级规则；不能只限制块对数量却放任每对无界 diff。
- [ ] 验证新块 ID 唯一、相似度一致、块拆并策略和 Agent 预期文本保护未退化。
- [ ] 为预算降级增加诊断计数；避免在正常文档路径上逐块大量打日志。

**验证：** editor 的计数边界与块身份测试同时跑 Debug / Release；完整种子属性集在慢速任务执行，保留原有质量阈值。

**完成条件：** A2；不能通过降低属性测试质量阈值消除回归。

## Task 3：同步协议身份、拒绝恢复与无窗口测试

**修改：** `crates/editor/src/{protocol,bridge}.rs`、`crates/desktop/web/src/{rust-sync,cm-engine,engine,app,harness}.ts`、web `package.json` / `tsconfig.json`；新增 `web/tests/` 与共享协议 fixture。

- [ ] 建立纯同步测试入口，使用 tsc 编译和 Node 内建测试运行器；新增 `npm test`，不加载真实窗口或引入浏览器自动化依赖。
- [ ] 先复现 sent + queued 遇到非 stale 拒绝时等待悬挂；覆盖无 queued 的拒绝、失序 / 重复确认、非法回包和超时。
- [ ] 协议增加版本、request ID、结构化错误码；TS 改成判别联合类型并校验入站字段，Rust / TS 消费同一组 JSON fixture。
- [ ] 增加 normal 模式只读快照请求，保持 nonce / generation 和 SelfTest 权限边界；最近重复请求幂等确认，同 ID 异内容报错。
- [ ] 实现 spec §6 状态机，保留最后确认基线、sent、queued 与当前文本；无完整变更链时停止自动重试并请求快照。
- [ ] 恢复不能证明安全时进入 conflict；展示可复制 / 导出的本地内容，显式选择后才放弃草稿或作为新修改提交。
- [ ] 为 settled 增加类型化失败 / 关闭结果，注入时钟测试超时；关闭与恢复失败必须结算全部 waiter。
- [ ] 测试恢复过程中输入、Agent 更新、重复请求、过时回包、取消、错误 nonce 与协议版本不兼容，确认无静默丢字或重复应用。
- [ ] 同次重建并提交嵌入页面，不让新 Rust 协议配旧 dist；同步更新 harness 断言。

**验证：** editor bridge 测试；web `npm run check`、新增 `npm test`、`npm run fixtures:check`；`bash scripts/build-editor-web.sh --check`；具备图形会话时跑 `bash scripts/run-editor-selftest.sh`。

**完成条件：** A3；所有异常均收敛为确认或可见终态，normal 模式没有新增任意 Load 权限。

## Task 4：清理台账读取分级与持久化失败可见

**修改：** `crates/app-host/src/cleanups.rs`、`crates/desktop/src/apps/{store,mod,management}.rs` 及设置 Apps 实际展示接缝。

- [ ] 先补损坏 / I/O 读取失败、request 写入失败后重复 request、finish / fail 写入失败的测试；替换现有“损坏就是空表”预期。
- [ ] load 返回类型化结果；missing 可空，其余错误保留原文件和 unavailable 状态，不能在后续 request 时覆盖不可读来源。
- [ ] 可用队列增加 dirty / last_error 状态，所有变更的持久化失败向产品返回并可重试；相同内容重试也真正执行 persist。
- [ ] 抽出最小故障注入接缝验证 write / sync / rename 失败；同目录唯一临时文件写入并同步，失败不损坏原文件。
- [ ] 设置 Apps / 应用管理显示读取故障、未落盘意图与重试结果；其他应用功能继续可用，不能显示清理成功。
- [ ] 保留卸载成功后才清理、webview 离池、InUse 退避、Unsupported / Failed 可见等行为。
- [ ] 新提交仍未落盘时，退出流程报告未完成意图；损坏来源只允许明确恢复 / 重试，不做猜测性目录删除。

**验证：** `cargo test --locked -p digger-app-host cleanups`；desktop store / ProgramAndData / 设置相关回归，损坏文件内容与故障前原文件逐字节核对。

**完成条件：** A4；读取故障不再表现为空队列，持久化失败可见且同一意图可重试。

## Task 5：编辑器创建状态与实际重试

**修改：** `crates/desktop/src/markdown/{mod,host,tests}.rs`、相关生命周期测试。

- [ ] 将创建状态明确为 Idle / Building / Ready / Failed / Closed，可见性与几何独立维护。
- [ ] 增加失败后重试消息及真实执行路径；推进 generation、清理旧句柄、清除旧错误后再构建。
- [ ] 覆盖迟到 Built 成功 / 失败、旧 Page、连续重试、创建途中关闭、隐藏后恢复与退出。
- [ ] 失败时文案对应实际可触发动作；Ready 隐藏 / 显示不重建页面或撤销栈。
- [ ] 与 Task 3 的同步恢复配合，禁止创建重试隐式丢弃未处理草稿。

**验证：** markdown 状态测试；`bash scripts/run-editor-selftest.sh lifecycle`；实机注入一次创建失败，再通过产品入口重试，记录句柄与 generation。

**完成条件：** A5。阶段 R 到此可独立验收，未完成实机项必须标注。

## Task 6：可重复完整路径基准与扫描优化

**新增：** editor 基准入口（例如 `crates/editor/examples/sync_bench.rs`）、web 基准入口、固定语料 / 种子与结果格式。
**修改：** `crates/editor/src/doc.rs`、`crates/desktop/src/markdown/mod.rs`。

- [ ] 固化 Task 0 语料，基准分别测 Doc、Bridge + JSON、前端应用和真实 webview；保留基线原始样本，不将临时探针数值直接作为门禁。
- [ ] 有序修改端点一次转换为字节坐标，保留 surrogate 边界和排序 / 不重叠检查。
- [ ] 建立按版本维护的长度 / 字符统计；增删片段更新，load / restore / Agent 回滚可完整重建。
- [ ] 用有序块边界顺序生成范围，去掉不必要的排序和 HashMap 分配。
- [ ] 顶栏读缓存字符数，避免每次 view 扫描全文；普通确认不再要求全文 FNV，显式检查与快照继续完整校验。
- [ ] 测试缓存与参考实现等价，覆盖 emoji、空文档、文末插入、多端点替换、计划回滚与状态导入导出。

**验证：** editor 精确 / 属性测试；Release 同环境前后基准；相关 web harness。Task 6 / 7 联合达到 A6–A7，不能只报告 parser 耗时。

## Task 7：块增量结果与前端应用

**修改：** `crates/editor/src/{doc,protocol,bridge}.rs`、`crates/desktop/web/src/{rust-sync,cm-engine,harness}.ts`、协议 fixture、嵌入 bundle。

- [ ] 冻结 baseVersion / resultVersion、旧区间坐标、结果块坐标、保留块映射次序及 how 语义；作为协议 fixture 的可执行契约。
- [ ] 从 reparse_window 产出变化区间；跨区编辑扩大区间，无法证明时显式回退完整快照。
- [ ] 前端先更新确认块集合，再映射 sent / queued，覆盖删除 / 拆并、邻居边界、标题 / 类型变化、Agent 并发和重复确认。
- [ ] 对固定和随机编辑流同时运行“应用增量”和“完整结果”参考路径，逐版本比对文本、ID、范围、元数据及版本。
- [ ] 错版本、缺块、范围越界进入 Task 3 恢复；不继续接收后续增量掩盖错误。
- [ ] 重建 dist，运行完整 harness，测量 spec §7.3 的回包大小、Rust p95 和真实页面结果；未达标保留数据并继续定位。

**验证：** bridge / protocol 测试、web 无窗口状态测试、fixture 与 bundle 一致性；Release 基准。固定环境满足 5000 块局部确认字节下降至少 90%、Rust p95 改善至少 2 倍、小语料无稳定超过 10% 的回退。

**完成条件：** A6–A7。不在此任务顺便引入 Rope 或通用编辑器 actor。

## Task 8：SQLite 用量账本与旧格式导入

**修改：** `crates/store/src/{ledger,service,lib}.rs`，按需要新增 `usage_db.rs` / `usage_import.rs`；`crates/paths/src/lib.rs`、`crates/desktop/src/paths.rs`。

- [ ] 增加 `usage_db_path()` 到独立 `state/usage.sqlite3`，保留旧 JSON 路径供导入；测试 DIGGER_DATA_DIR 和产品隔离。
- [ ] 创建版本化 schema、run_id 主键、项目 / 结束时间索引及完整 payload；显式处理 u64 / SQLite 整数范围，保留全部 DTO 字段。
- [ ] 配置并验证 WAL、有限 busy timeout 和 synchronous=FULL；连接关闭与 checkpoint 由后台 owner 执行。
- [ ] 实现事务 upsert 和索引查询，保留项目精确过滤、毫秒单位、since 边界与同 run_id 累计值覆盖。
- [ ] 旧 JSON 校验、导入与完成标记在同一事务；同 ID 同内容可复用，不同内容不覆盖，损坏源保持原样。
- [ ] 提交后才归档；归档失败按标记恢复；已有归档禁止覆盖。不采用新旧双写。
- [ ] 验证导入中止、提交后中止、重复启动、目标已有数据、范围异常、空 / 缺失 / 损坏 JSON，保证失败不表现为空账本成功。
- [ ] 提供停写后一致性导出到旧 JSON 的回退工具与校验，或者明确阻止不支持的数据回退；保留 SQLite 与源备份。

**验证：** store 与 paths 相关测试；迁移中止使用临时目录和子进程；查询计划确认命中索引，10 万记录语料验证 upsert 不再重写全部历史。

**完成条件：** A8；旧格式来源可追溯，完整字段 round-trip 无损，文档写明回退条件。

## Task 9：后台账本服务、失败重试与退出排空

**修改：** `crates/store/src/{runner,service}.rs` 及后台 worker 模块、`crates/desktop/src/apps/mod.rs`、product 启动 / 退出接线、用量错误展示。

- [ ] 单工作线程拥有同步 SQLite 连接，暴露有界写入 / 查询命令；不在窗口线程或 Tokio worker 中直接执行长时间数据库调用。
- [ ] 用完成事件而非 RunningTask 重试；结束时刻计算一次，run_id / 累计值稳定。pending、persisted、failed 结果回到产品消息。
- [ ] 移除 `Arc<Mutex<UsageLedger>>` 的产品直接操作，保留 UsageService 的消费者契约；只查询已提交事实。
- [ ] 注入慢写、锁库、磁盘错误和队列满，验证 update 不等待、事件不静默丢失、错误可见且可按同一事件重试。
- [ ] 正常退出停新提交、排空并等待确认；限时失败进入可操作退出状态，强制退出不伪称未确认记录已保存。
- [ ] 使用 1000 / 10000 / 100000 记录比较写入和查询，确认历史增长不再触发全量 clone / pretty JSON 重写。

**验证：** store worker / runner / service 测试、desktop 用量与退出集成测试；延迟由可控 barrier / fake 存储注入，不用脆弱毫秒断言证明非阻塞。

**完成条件：** A9；后台提交失败不能以可查询内存行冒充成功。

## Task 10：组合根按职责拆分

**修改：** `crates/desktop/src/product.rs`；按实际依赖新增 `product/{routing,subscriptions,native_sync,boot}.rs` 或等价模块，迁移相关测试。

- [ ] 先列出状态 owner、消息路由、订阅 identity、原生视图同步与展示快照的依赖清单。
- [ ] 逐项移动纯协调函数；ProductShell 保持唯一状态所有者，主入口保持可读的装配顺序。
- [ ] 保留 Project 归属、过时结果过滤、后台完成事实、窗口 / 设置路由和焦点快照刷新次序。
- [ ] 不通过 pub 暴露所有内部字段解决模块依赖；不用新的动态消息总线替代现有类型化消息。
- [ ] 纯移动与行为变化分开提交；迁移现有测试，不为验证文件名或行数新增测试。

**验证：** desktop 的 product / review / chat / conversations 等相关回归、所有 examples 编译、编辑器 / 用量生命周期自检；针对改动接缝检查焦点、主题和 Project 切换。

**完成条件：** A10；按职责减少组合根协调负担，不以行数阈值作为唯一验收。

## Task 11：快速 CI、慢速验证与产物一致性

**修改：** `.github/workflows/consumer-check.yml`，新增按需命名的慢速 workflow；editor 慢速测试组织、web 测试脚本、README 验证说明。

- [ ] PR job 增加 core Rust 行为测试、Node 工具链、npm ci / check / test、fixture 和 `build-editor-web.sh --check`，保留全部现有依赖门禁。
- [ ] 给真正耗时的书籍 / 全种子属性测试建立明确慢速入口，提供快速代表集；不能只把失败或耗时测试全部 ignore。
- [ ] 定时 / 手动 job 运行完整属性集和 Release 基准，上传失败种子、统计和性能样本。
- [ ] 图形 / 进程测试在具备能力的 macOS 环境运行，记录 ps 权限、屏幕 / GUI 条件和生产 webview 自检结果；环境不足列为未验证。
- [ ] 核对 macOS runner 的 shell / Node 版本与脚本兼容性，清理 workflow 中与现状不符的历史说明。
- [ ] 验证缓存键覆盖 Cargo.lock 与 web lock；构建产物检查确实能拦下源码 / dist 不一致，报告区分 compile、行为、慢速和实机结果。

**验证：** 从干净环境执行 PR 命令集；手动触发慢速验证或本地等价执行并记录结果。可用时用一次临时错误 fixture 验证门禁，再恢复，不提交破坏性夹具。

**完成条件：** A11；不把环境失败、未结束测试或仅编译成功记录为行为通过。

## Task 12：阶段验收、无 patch 消费与回退说明

**新增：** `docs/superpowers/reviews/2026-11-02-code-optimization-acceptance.md`；更新本 spec / plan 状态和 README 链接。

- [ ] 对照 A1–A12 逐项附测试、命令、退出状态、固定种子、性能原始数据和实机证据，标明各阶段状态。
- [ ] 在隔离 checkout / HOME / Cargo 配置中验证声明依赖，保留用户开发配置；共享依赖不可访问时列出明确阻碍，不宣称发行通过。
- [ ] 运行 `cargo fmt --all --check`、`cargo check --locked --workspace --all-targets`、原有依赖门禁、快速 / 慢速 / 实机相关检查；核对无意外 lock 或其他仓库改动。
- [ ] 同时验证不同临时产品根，确认项目迁移、usage、清理台账互不串扰，未访问真实用户数据。
- [ ] 整理迁移恢复操作、JSON / SQLite 备份、旧版本回退工具或限制；新数据存在时禁止用旧来源覆盖。
- [ ] 记录尚未解决的性能瓶颈。只有证据支持时另开 Rope / 后台文档 actor 任务，不擅自扩大当前变更。
- [ ] 实施验收完成才勾选任务并改状态；发布、tag 和跨仓库修改另按届时任务范围执行。

**完成条件：** A12；全部阶段完成时 A1–A12 均有证据。阶段 R 独立交付时保留 P / E 未完成标记。

## Spec 覆盖索引

| Spec / 验收 | 任务 |
|---|---|
| §2 基线与证据 | Task 0、6、12 |
| §4 迁移 / A1 | Task 1 |
| §5 统计与预算 / A2 | Task 2 |
| §6 同步恢复 / A3 | Task 3、7 |
| §9.1 清理 / A4 | Task 4 |
| §9.2 创建重试 / A5 | Task 5 |
| §7 坐标与增量性能 / A6–A7 | Task 6、7 |
| §8 导入、后台存储 / A8–A9 | Task 8、9 |
| §10 结构与 CI / A10–A11 | Task 10、11 |
| §12 交付 / A12 | Task 12 |
