# Digger 与本地 ByteUI 联调

2026-10-31：远端 ByteUI 最新 tag 仍为 `v0.5.0`，本地 `../byteui` 有未发布接口。Digger 工作区的原生菜单、主题快照、面板标题和动画暂停接线已消费这些接口，单独使用远端 `v0.5.0` 不能编译这些接线。

本机 `.cargo/config.toml`（已忽略、不提交）增加 ByteUI path patch，沿用已有 Dozer path patch。Cargo.lock 随本地解析更新；包含 path 来源的锁文件只适用于联调，不能作为正式发布锁文件。未修改兄弟仓库。

验证：

- `CARGO_TARGET_DIR=target cargo check --workspace --all-targets --offline` 通过。
- `cargo fmt --all --check` 通过。
- `bash scripts/check-host-deps.sh` 通过。
- 桌面库测试：715/716 通过；`apps::tests::closing_during_startup_waits_and_releases_the_root` 在整组运行失败，单独原命令过滤重跑通过，整组结果仍记为失败。

正式发布前，需要 ByteUI 发布包含这些接口的新 tag，并与 Dozer 共享库快照统一钉版，然后移除本地 patch、重新生成远端锁文件，运行 shared-deps 守卫和完整消费者编译。当前不把未发布接口标记为已完成远端版本对齐。
