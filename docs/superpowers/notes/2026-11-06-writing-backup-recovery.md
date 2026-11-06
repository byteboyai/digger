# 写字台备份、恢复与回退

适用范围：W1 Writing Domain。正文权威来源为 `.digger/project.db`，不能以可能滞后的 `.md` 镜像替代数据库备份。

## 操作

1. 在写字台保存，或在关闭提示中选择“保留恢复草稿后继续”；关闭所有访问该项目的 Digger 实例及其它写入进程。退出提示失败时先处理错误，不能把尚未确认的页面输入当作已备份。
2. 在仓库正式依赖环境运行以下命令，目标必须是不存在的新目录：

   ```sh
   cargo run --locked -p digger-store --example writing_backup -- backup /absolute/project /absolute/new-backup
   cargo run --locked -p digger-store --example writing_backup -- restore /absolute/new-backup /absolute/new-project
   ```

3. 打开恢复后的新项目，先核对文稿及恢复草稿，再继续写作。源项目及备份不会被恢复命令覆盖。

## 包含的事实

- SQLite 使用 `VACUUM INTO` 生成一致快照，包含尚在 WAL 中的已提交记录、版本、块身份、Commit、项目 revision、Outbox 和幂等结果。禁止运行中直接复制 `project.db` 而遗漏 WAL。
- `markdown/` 从该快照的权威正文生成，保留记录的 BOM/换行；即使 Outbox 未排空，也有可带走的最新已提交 Markdown。
- `external/` 保留受管理文件的实际磁盘字节，包含尚未解决的外部改动；恢复时把这些字节放回原相对路径。镜像不同步时用“比较文件”处理，外部副本与权威导出分别保留。
- `.digger/working/` 中的 JSON 草稿与遗留临时文件保留。manifest 记录每个交付文件的 SHA-256；摘要失败、非法路径、链接、重复目标或不支持的 schema 会终止恢复，不发布半成品。
- 恢复到新目录时重建 store UUID；有效草稿重新绑定新 UUID，文稿 ID、块 ID、版本和幂等事实保持。损坏/未来格式的草稿保留原件并提示，不自动接受。

数据库快照具有 SQLite 一致性；数据库、恢复文件与外部文件之间没有跨介质快照事务，因此收集这些文件前必须关闭其它写者。W1 尚无托管图片/资源模型，这不是整个项目任意文件的通用备份；图片及其它项目文件需另行备份。共享 `projects.db` 注册表也不在文稿备份内，恢复后通过“打开项目”重新注册。

## 故障与旧版本

- 写失败、满盘或只读时原项目不变；备份/恢复先写同级临时目录，再发布到新目标。失败可清理临时目录后用新目标重试。
- 不支持在其它连接运行时只替换主数据库、留下旧 WAL/SHM 的操作。这不是合法恢复；实例变化检测会拒绝旧会话继续保存/镜像，不能把已混合的文件集当作新库。关闭所有写者后使用一致备份恢复。
- future database schema 不降级、不交给旧二进制写入；原备份保持不变，使用支持该 schema 的版本恢复。已生成的 `markdown/` 可以交给其它编辑器使用，但不携带 Digger 的全部块身份及版本历史。
- 恢复不会清空或假装同步既有 Outbox。每次启动及空闲维护按 8 条处理（空闲维护间隔至少 5 秒，失败最多自动尝试 8 次，之后需明确重试），外部冲突持续阻塞同一文稿后续镜像；可另存外部副本或在重新核对摘要后明确确认替换。

演练证据：`writing::tests::backup_restore_preserves_wal_data_recovery_external_files_and_forks_identity`，同时覆盖已打开 WAL 库、恢复草稿、外部改动、权威导出、重开、新 UUID、已有目标拒绝和摘要损坏拒绝。
