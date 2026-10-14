//! 桥协议(ADR-013 §5):页面 ↔ Rust 的类型化消息。线上格式与 spike 已验证的 T11–T23 一致,位置一律是
//! UTF-16 码元,`hash` 是 UTF-8 文本的 FNV-1a。未知 / 解析不了的入站消息一律回 `reject`,绝不静默丢弃
//! (spike 发现:一条静默丢掉的消息会让编辑器永远等下去)。

use serde::{Deserialize, Serialize};

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
    /// 页面就绪,请 Rust 给出文档。
    Ready {
        #[serde(default)]
        engine: Option<String>,
    },
    /// 一批编辑,坐标是页面在 `base` 版本时的文档。
    Edit { base: u64, changes: Vec<Change> },
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
        matches!(self, Self::Load { .. } | Self::Agent { .. } | Self::Result { .. } | Self::Info { .. } | Self::Done {})
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
    Open { text: String, version: u64, hash: String, len16: usize, blocks: Vec<BlockInfo> },
    /// 编辑被接受后的块列表。
    Blocks { version: u64, hash: String, len16: usize, blocks: Vec<BlockInfo> },
    /// Agent 补丁:改动 + 结果版本与块。
    Update { origin: String, changes: Vec<Change>, version: u64, hash: String, len16: usize, blocks: Vec<BlockInfo> },
    /// Agent 步骤没有应用(正常结果,不是故障)。
    AgentError { reason: String, stale: bool },
    /// 消息被拒绝(过期的 base、解析不了、未知、nonce 不对……)。
    Reject { reason: String, version: Option<u64> },
}

impl EditorOutbound {
    /// 线上 JSON。
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("outbound message serialises")
    }
}
