//! digger-editor:Markdown 编辑器的 Rust 一侧(ADR-012 / ADR-013)。
//!
//! 业务 crate:**不得**依赖 `bytehost-*`、`iced*`、`wry`(`scripts/check-host-deps.sh` 把关)。
//! 它只管文本、块、块 ID、同步规则;窗口、webview、Surface 都在宿主适配层(`digger-desktop`)。

pub mod blocks;
pub mod doc;
pub mod protocol;
pub mod bridge;
