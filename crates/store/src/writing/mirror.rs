//! Markdown 镜像:把权威正文按记录的新行/BOM 风格原子写入项目文件。
//!
//! 数据库提交不等待镜像成功;这里只负责一次受控写入。调用方根据 Outbox
//! 记录决定重试、阻塞（外部修改）或确认。

use dozer_file_io::{AtomicWriteError, AtomicWriteOutcome, ProjectRoot, Revision, WriteOptions};

use super::types::{NewlineStyle, WritingError, validate_relative_path};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceText {
    /// 已去掉 BOM、换行已规范为 LF 的正文。
    pub text: String,
    pub newline: NewlineStyle,
    pub bom: bool,
    /// 原始源文件字节摘要(含 BOM),作为首次镜像的明确基线。
    pub file_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MirrorBaseline {
    /// 旧 Outbox 的兼容模式:不校验当前文件。
    Any,
    /// 新建文稿的首次镜像:目标必须不存在。
    Absent,
    /// 导入或已同步版本的明确摘要:当前文件必须一致。
    Digest(String),
}

/// 读取项目内 Markdown 源文件:拒绝项目外/符号链接,校验 UTF-8 与大小,
/// 记录 BOM / CRLF 风格但不做其它 Markdown 规范化。
pub fn read_source(
    project_root: &std::path::Path,
    relative: &str,
) -> Result<SourceText, MirrorError> {
    let relative = validate_relative_path(relative)?;
    let root = ProjectRoot::open(project_root)
        .map_err(|error| MirrorError::Io(format!("打开项目根失败: {error}")))?;
    let opened = root.open_file(&relative).map_err(|error| match error {
        dozer_file_io::FileError::NotFound => MirrorError::Io("源文件不存在".into()),
        other => MirrorError::Io(other.to_string()),
    })?;
    let bytes = opened
        .read_limited(super::types::MAX_TEXT_BYTES)
        .map_err(|error| MirrorError::Io(error.to_string()))?;
    let file_digest = digest_bytes(&bytes);
    let (bom, bytes) = match bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        Some(rest) => (true, rest),
        None => (false, bytes.as_slice()),
    };
    let raw = std::str::from_utf8(bytes)
        .map_err(|error| MirrorError::Io(format!("源文件不是 UTF-8: {error}")))?;
    let newline = if raw.contains("\r\n") {
        NewlineStyle::Crlf
    } else {
        NewlineStyle::Lf
    };
    let text = raw.replace("\r\n", "\n");
    Ok(SourceText {
        text,
        newline,
        bom,
        file_digest,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirrorOutcome {
    pub file_digest: String,
    pub durability_unknown: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MirrorError {
    ExternalChange(String),
    InvalidPath(String),
    Io(String),
    DurabilityUnknown {
        warning: String,
        file_digest: String,
    },
}

impl std::fmt::Display for MirrorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ExternalChange(reason) => write!(f, "外部文件已变化: {reason}"),
            Self::InvalidPath(reason) => write!(f, "非法镜像路径: {reason}"),
            Self::Io(reason) => write!(f, "镜像写入失败: {reason}"),
            Self::DurabilityUnknown { warning, .. } => {
                write!(f, "镜像已替换但持久性未确认: {warning}")
            }
        }
    }
}

impl std::error::Error for MirrorError {}

impl From<WritingError> for MirrorError {
    fn from(error: WritingError) -> Self {
        match error {
            WritingError::InvalidPath(reason) => Self::InvalidPath(reason),
            other => Self::Io(other.to_string()),
        }
    }
}

/// 库内正文是 LF;镜像按来源风格恢复。BOM 只在文件头加一次。
pub fn encode_mirror(text: &str, newline: NewlineStyle, bom: bool) -> Vec<u8> {
    let encoded = newline.encode(text);
    let mut bytes = Vec::with_capacity(encoded.len() + usize::from(bom) * 3);
    if bom {
        bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    }
    bytes.extend_from_slice(encoded.as_bytes());
    bytes
}

pub fn digest_bytes(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// 读取当前镜像摘要,用于外部修改检测（不跟随符号链接）。
pub fn current_file_digest(
    root: &ProjectRoot,
    relative: &str,
) -> Result<Option<String>, MirrorError> {
    let relative = validate_relative_path(relative)?;
    match root.open_file(&relative) {
        Ok(opened) => {
            let bytes = opened
                .read_limited(super::types::MAX_TEXT_BYTES)
                .map_err(|error| MirrorError::Io(error.to_string()))?;
            Ok(Some(digest_bytes(&bytes)))
        }
        Err(dozer_file_io::FileError::NotFound) => Ok(None),
        Err(error) => Err(MirrorError::Io(error.to_string())),
    }
}

/// 原子写入镜像。`expected_file_digest` 是调用方看到的基线摘要:
/// - `None` 表示允许首次创建;
/// - `Some` 时当前文件必须存在且摘要一致,否则返回 [`MirrorError::ExternalChange`]。
pub fn write_mirror(
    root: &ProjectRoot,
    relative: &str,
    text: &str,
    newline: NewlineStyle,
    bom: bool,
    baseline: &MirrorBaseline,
) -> Result<MirrorOutcome, MirrorError> {
    let relative = validate_relative_path(relative)?;
    let bytes = encode_mirror(text, newline, bom);
    let file_digest = digest_bytes(&bytes);

    // 新建文稿可能带子目录;目录本身必须先在项目根内创建,最终文件仍由
    // `ProjectRoot` 的 O_NOFOLLOW 路径检查打开。
    if let Some(parent) = std::path::Path::new(&relative).parent()
        && !parent.as_os_str().is_empty()
    {
        let mut directory = root.canonical_path().to_owned();
        for component in parent.components() {
            directory.push(component.as_os_str());
            match std::fs::symlink_metadata(&directory) {
                Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {}
                Ok(_) => {
                    return Err(MirrorError::InvalidPath(
                        "镜像父目录包含链接或非常规目录".into(),
                    ));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    std::fs::create_dir(&directory).map_err(|e| MirrorError::Io(e.to_string()))?
                }
                Err(error) => return Err(MirrorError::Io(error.to_string())),
            }
        }
    }

    let expected_revision = match root.open_file(&relative) {
        Ok(opened) => {
            let revision = Revision::of(&opened.metadata);
            let current = opened
                .read_limited(super::types::MAX_TEXT_BYTES)
                .map_err(|error| MirrorError::Io(error.to_string()))?;
            let current_digest = digest_bytes(&current);
            // A crash may have happened after rename but before Outbox ACK.
            // Identical bytes need no write and can safely acknowledge replay.
            if current_digest == file_digest {
                return Ok(MirrorOutcome {
                    file_digest,
                    durability_unknown: false,
                });
            }
            match baseline {
                MirrorBaseline::Absent => {
                    return Err(MirrorError::ExternalChange(
                        "新建文稿的目标文件在本程序创建前已存在".into(),
                    ));
                }
                MirrorBaseline::Digest(expected) if expected != &current_digest => {
                    return Err(MirrorError::ExternalChange(format!(
                        "基线 {expected} 与当前 {current_digest} 不一致"
                    )));
                }
                _ => {}
            }
            Some(revision)
        }
        Err(dozer_file_io::FileError::NotFound) => {
            if let MirrorBaseline::Digest(expected) = baseline {
                return Err(MirrorError::ExternalChange(format!(
                    "基线 {expected} 对应的文件已不存在"
                )));
            }
            None
        }
        Err(error) => return Err(MirrorError::Io(error.to_string())),
    };

    let options = WriteOptions {
        lock_root: None,
        expected_revision,
        create: true,
        lock_timeout: std::time::Duration::from_secs(5),
    };
    match dozer_file_io::write_atomic(root, &relative, &bytes, &options) {
        Ok(AtomicWriteOutcome::Committed { .. }) => Ok(MirrorOutcome {
            file_digest,
            durability_unknown: false,
        }),
        Ok(AtomicWriteOutcome::CommittedDurabilityUnknown { warning, .. }) => {
            Err(MirrorError::DurabilityUnknown {
                warning,
                file_digest,
            })
        }
        Err(AtomicWriteError::CommittedDurabilityUnknown { warning, .. }) => {
            Err(MirrorError::DurabilityUnknown {
                warning,
                file_digest,
            })
        }
        Err(AtomicWriteError::NotCommitted(error)) => Err(MirrorError::Io(error.to_string())),
    }
}

/// The exact candidate shown to the user. Confirmation is valid only while both
/// the durable revision and the source bytes still match this review.
#[derive(Debug, Clone)]
pub struct ExternalReview {
    pub document: super::Document,
    pub official: super::WritingSnapshot,
    pub candidate: Option<SourceText>,
}
