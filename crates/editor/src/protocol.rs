//! 桥协议(ADR-013 §5):页面 ↔ Rust 的类型化消息。线上格式与 spike 已验证的 T11–T23 一致,位置一律是
//! UTF-16 码元,`hash` 是 UTF-8 文本的 FNV-1a。未知 / 解析不了的入站消息一律回 `reject`,绝不静默丢弃
//! (spike 发现:一条静默丢掉的消息会让编辑器永远等下去)。

use serde::{Deserialize, Serialize};

/// 线上协议版本。Rust 与编译进二进制的页面同次更新;版本不匹配时停止同步,不按旧格式继续。
pub const PROTOCOL_VERSION: u32 = 2;

/// 结构化错误码。状态机只按 code 分支,`reason` 只用于展示。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// 编辑的 base 不是当前版本(通常 Agent 先改了文档)。
    StaleBase,
    /// UTF-16 位置落在代理对中间或越界。
    BadPosition,
    /// changes 未排序 / 重叠 / 格式不合法。
    MalformedChanges,
    /// nonce / 模式不允许该消息(例如 normal 模式触发 Load / Agent)。
    Unauthorized,
    /// 协议版本不兼容。
    UnsupportedProtocol,
    /// 同一 request ID 携带了不同内容。
    DuplicateRequest,
    /// request ID 早于最近确认的请求,无法判断是否重复。
    OutOfOrderRequest,
    /// 内部错误。
    Internal,
}

/// 一处文本改动(UTF-16 位置)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    pub from: usize,
    pub to: usize,
    pub insert: String,
}

/// 页面 → Rust。带握手 nonce 的信封见 [`Envelope`]。
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum EditorInbound {
    /// 页面就绪,请 Rust 给出文档。`protocol` 缺省视为 1(旧页面),不匹配当前版本会被拒绝。
    Ready {
        #[serde(default)]
        protocol: Option<u32>,
        #[serde(default)]
        engine: Option<String>,
    },
    /// 一批编辑,坐标是页面在 `base` 版本时的文档。`request_id` 在会话内单调递增。
    Edit {
        #[serde(default)]
        request_id: Option<u64>,
        base: u64,
        changes: Vec<Change>,
    },
    /// 只读快照:normal 模式允许,用于拒绝 / 超时后的恢复;不改文档,也不扩大写权限。
    Snapshot {
        #[serde(default)]
        request_id: Option<u64>,
    },
    /// 仅 harness:页面自带语料,请 Rust 当作新文档载入。
    Load { text: String },
    /// 仅 harness:代替 Agent 运行时的测试入口,按块 id 改文本。
    Agent {
        op: String,
        #[serde(default)]
        id: String,
        #[serde(default)]
        text: String,
        #[serde(default)]
        expect: Option<String>,
    },
    /// 仅 harness:一项检查的结果。
    Result {
        name: String,
        pass: bool,
        #[serde(default)]
        detail: String,
    },
    /// 仅 harness:信息行。
    Info {
        name: String,
        #[serde(default)]
        detail: String,
    },
    /// 仅 harness:全部检查结束。
    Done {},
}

impl EditorInbound {
    /// 只在自检模式下接受的消息;正常运行时同未知消息一样被拒绝。
    pub fn selftest_only(&self) -> bool {
        matches!(
            self,
            Self::Load { .. }
                | Self::Agent { .. }
                | Self::Result { .. }
                | Self::Info { .. }
                | Self::Done {}
        )
    }
}

/// 信封:每条上行消息都带初始化脚本注入的 nonce,不带或不对的一律拒绝。
#[derive(Debug, Deserialize)]
pub struct Envelope {
    pub n: String,
    #[serde(flatten)]
    pub msg: EditorInbound,
}

/// 一个顶层块,位置是 UTF-16。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BlockInfo {
    pub id: String,
    pub kind: &'static str,
    pub level: Option<u8>,
    pub title: Option<String>,
    pub how: &'static str,
    pub from: usize,
    pub to: usize,
    pub head: String,
}

/// Rust → 页面。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum EditorOutbound {
    /// 握手的回答:初始文档。
    Open {
        protocol: u32,
        text: String,
        version: u64,
        hash: String,
        len16: usize,
        blocks: Vec<BlockInfo>,
    },
    /// 编辑被接受后的块列表(带对应的 request ID,便于丢弃过时回包)。
    Blocks {
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<u64>,
        version: u64,
        hash: String,
        len16: usize,
        blocks: Vec<BlockInfo>,
    },
    /// 局部编辑的块增量:替换 `base_version` 块列表的 `[removed_from, removed_to)` 区间,
    /// 新块的 `from`/`to` 已是在结果版本文本里的坐标;保留块先由 `changes` 映射，how 置为 Exact。
    Incremental {
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<u64>,
        base_version: u64,
        version: u64,
        hash: String,
        len16: usize,
        changes: Vec<Change>,
        removed_from: usize,
        removed_to: usize,
        blocks: Vec<BlockInfo>,
    },
    /// 只读快照(Rust 权威文本 + 块)。恢复交互用它决定是否采用权威版本。
    Snapshot {
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<u64>,
        text: String,
        version: u64,
        hash: String,
        len16: usize,
        blocks: Vec<BlockInfo>,
    },
    /// Agent 补丁:改动 + 结果版本与块。
    Update {
        origin: String,
        changes: Vec<Change>,
        version: u64,
        hash: String,
        len16: usize,
        blocks: Vec<BlockInfo>,
    },
    /// Agent 步骤没有应用(正常结果,不是故障)。
    AgentError { reason: String, stale: bool },
    /// 消息被拒绝(过期的 base、解析不了、未知、nonce 不对……)。
    Reject {
        code: ErrorCode,
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<u64>,
        reason: String,
        version: Option<u64>,
    },
}

impl EditorOutbound {
    /// 线上 JSON。
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("outbound message serialises")
    }
}
