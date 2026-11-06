//! Writing Domain(W1):项目内 `.digger/project.db` 的文稿仓库、恢复草稿与镜像。
//!
//! 只做业务事实与文件协调,不依赖任何宿主 UI。UI 通过 `digger-store` 的 typed
//! API 取快照/提交;`digger-editor` 只提供纯 `State` 序列化,不感知 SQLite。

mod backup;
mod mirror;
mod recovery;
mod store;
mod types;

pub use backup::{backup_writing, restore_writing};
pub use mirror::{
    ExternalReview, MirrorBaseline, MirrorError, MirrorOutcome, SourceText, current_file_digest,
    digest_bytes, encode_mirror, read_source, write_mirror,
};
pub use recovery::{
    MAX_RECOVERY_BYTES, RECOVERY_SCHEMA, RecoveryClass, RecoveryDraft, RecoveryRead, RecoveryStore,
    classify,
};
pub use store::{PROJECT_DB_RELATIVE, SCHEMA_VERSION, WritingStore};
pub use types::{
    CreateOutcome, Document, DocumentId, LoadedDocument, MAX_STATE_BYTES, MAX_TEXT_BYTES,
    NewDocument, NewlineStyle, OutboxEntry, SaveReceipt, SaveRequest, StoreId, WritingError,
    WritingSnapshot, validate_relative_path,
};

#[cfg(test)]
mod tests;
