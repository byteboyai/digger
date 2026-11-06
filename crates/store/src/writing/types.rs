//! 文稿领域的纯值类型:身份、快照、保存请求/回执与结构化错误。
//!
//! 这些类型可以跨线程、可以 serde;不暴露 SQLite `Connection`,也不依赖宿主 UI。

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// 文稿库实例身份。写入 `<project>/.digger/project.db` 的 meta 表,项目目录被复制/
/// 移动时随库文件一起走;宿主用它识别“同一个库”而不是只看路径。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StoreId(pub String);

impl StoreId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// 稳定文稿身份(UUIDv7,创建后不因路径/标题变化而复用)。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DocumentId(pub String);

impl DocumentId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for DocumentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NewlineStyle {
    Lf,
    Crlf,
}

impl NewlineStyle {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "lf",
            Self::Crlf => "crlf",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "lf" => Some(Self::Lf),
            "crlf" => Some(Self::Crlf),
            _ => None,
        }
    }

    /// 库内正文一律 LF;镜像按记录恢复风格。
    pub fn encode(self, text: &str) -> String {
        match self {
            Self::Lf => text.to_owned(),
            Self::Crlf => text.replace('\n', "\r\n"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Document {
    pub id: DocumentId,
    pub title: String,
    pub relative_path: String,
    /// durable revision:已提交的正文字节/块身份版本。
    pub revision: u64,
    pub newline: NewlineStyle,
    pub bom: bool,
    pub created_ms: i64,
    pub updated_ms: i64,
}

/// 一次保存/导入的不可变快照。
///
/// `state_json` 是 `digger_editor::doc::State` 的序列化;store 不解析块语义,
/// 但会校验它是合法 JSON 且其中的 `text` 与 `text` 一致,避免正文与块身份错配。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WritingSnapshot {
    pub text: String,
    pub state_json: String,
    pub schema: u32,
}

impl WritingSnapshot {
    pub fn new(text: impl Into<String>, state_json: impl Into<String>, schema: u32) -> Self {
        Self {
            text: text.into(),
            state_json: state_json.into(),
            schema,
        }
    }

    /// 内容摘要覆盖正文、块身份与 schema,用于幂等/镜像/CAS 比较。
    pub fn digest(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update((self.schema as u64).to_le_bytes());
        hasher.update((self.text.len() as u64).to_le_bytes());
        hasher.update(self.text.as_bytes());
        hasher.update((self.state_json.len() as u64).to_le_bytes());
        hasher.update(self.state_json.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// 校验状态 JSON 与正文配对。正文可从 JSON 里省略;一旦存在就必须相等。
    pub fn validate(
        &self,
        max_text_bytes: usize,
        max_state_bytes: usize,
    ) -> Result<(), WritingError> {
        if self.text.len() > max_text_bytes {
            return Err(WritingError::Limit(format!(
                "正文 {} 字节超过上限 {max_text_bytes}",
                self.text.len()
            )));
        }
        if self.state_json.len() > max_state_bytes {
            return Err(WritingError::Limit(format!(
                "块身份状态 {} 字节超过上限 {max_state_bytes}",
                self.state_json.len()
            )));
        }
        if self.schema != digger_editor::doc::STATE_SCHEMA {
            return Err(WritingError::UnsupportedSchema {
                found: self.schema,
                supported: digger_editor::doc::STATE_SCHEMA,
            });
        }
        let state: digger_editor::doc::State =
            serde_json::from_str(&self.state_json).map_err(|error| {
                WritingError::InvalidSnapshot(format!("块身份状态不是合法 State: {error}"))
            })?;
        digger_editor::doc::validate_snapshot(&self.text, &state, self.schema)
            .map_err(WritingError::InvalidSnapshot)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewDocument {
    pub title: String,
    pub relative_path: String,
    pub snapshot: WritingSnapshot,
    pub newline: NewlineStyle,
    pub bom: bool,
    /// 导入源文件的字节摘要;`None` 表示新建,首次镜像要求目标不存在。
    pub source_file_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateOutcome {
    Created(Document),
    /// 同一相对路径已经存在:不覆盖、不复制,交给调用方引导“打开/导入候选”。
    Existing(Document),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveRequest {
    pub document: DocumentId,
    /// 调用方看到的 durable revision;不匹配即 CAS 冲突。
    pub expected_revision: u64,
    /// 幂等身份:同一个 operation id 重放只提交一次。
    pub operation_id: String,
    pub snapshot: WritingSnapshot,
    /// 镜像写入前的文件摘要基线;`None` 表示首次创建/没有外部基线。
    pub expected_file_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveReceipt {
    pub document: DocumentId,
    pub revision: u64,
    pub commit_id: i64,
    pub digest: String,
    /// true 表示命中幂等记录,本次没有产生新的提交。
    pub replayed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedDocument {
    pub document: Document,
    pub snapshot: WritingSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxEntry {
    pub id: i64,
    pub document: DocumentId,
    pub revision: u64,
    pub relative_path: String,
    pub expected_file_digest: Option<String>,
    pub target_digest: String,
    pub state: String,
    pub attempts: u32,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WritingError {
    NotFound(String),
    InvalidPath(String),
    InvalidSnapshot(String),
    Limit(String),
    RevisionConflict { expected: u64, actual: u64 },
    OperationConflict { operation_id: String },
    UnsupportedSchema { found: u32, supported: u32 },
    Corrupt(String),
    Unavailable(String),
}

impl std::fmt::Display for WritingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(what) => write!(f, "未找到{what}"),
            Self::InvalidPath(reason) => write!(f, "非法文稿路径: {reason}"),
            Self::InvalidSnapshot(reason) => write!(f, "非法文稿快照: {reason}"),
            Self::Limit(reason) => write!(f, "超过文稿上限: {reason}"),
            Self::RevisionConflict { expected, actual } => {
                write!(f, "文稿版本冲突:期望 {expected},实际 {actual}")
            }
            Self::OperationConflict { operation_id } => {
                write!(f, "操作 {operation_id} 已用不同内容提交过")
            }
            Self::UnsupportedSchema { found, supported } => write!(
                f,
                "文稿库 schema {found} 高于本程序支持的 {supported};只读拒绝写入"
            ),
            Self::Corrupt(reason) => write!(f, "文稿库损坏: {reason}"),
            Self::Unavailable(reason) => write!(f, "文稿库不可用: {reason}"),
        }
    }
}

impl std::error::Error for WritingError {}

pub const MAX_TEXT_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_STATE_BYTES: usize = 8 * 1024 * 1024;

/// 相对镜像路径的纯校验:非空、非绝对、无 `..`/根、拒绝内部 `.digger` 与空段。
pub fn validate_relative_path(path: &str) -> Result<String, WritingError> {
    if path.trim().is_empty() {
        return Err(WritingError::InvalidPath("路径为空".into()));
    }
    if path.starts_with('/') || path.starts_with('\\') {
        return Err(WritingError::InvalidPath("必须是项目内相对路径".into()));
    }
    let mut parts = vec![];
    for part in path.split('/') {
        if part.is_empty() || part == "." {
            return Err(WritingError::InvalidPath("路径含空段或 .".into()));
        }
        if part == ".." {
            return Err(WritingError::InvalidPath("路径不能包含 ..".into()));
        }
        if part.starts_with('.') && part == ".digger" {
            return Err(WritingError::InvalidPath(
                "不能写入 .digger 内部目录".into(),
            ));
        }
        parts.push(part);
    }
    Ok(parts.join("/"))
}
