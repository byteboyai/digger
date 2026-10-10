//! 桥接信封与入站校验(ADR-001 §5):绑定校验、封闭白名单、大小上限、seq 严格递增。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Binding {
    pub project_id: String,
    pub panel_id: String,
    pub document_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Envelope {
    pub host_kind: String,
    pub binding: Binding,
    pub seq: u64,
    #[serde(rename = "type")]
    pub ty: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, PartialEq)]
pub enum Reject {
    TooLarge,
    Malformed,
    WrongHostKind,
    ForeignBinding,
    UnknownType,
    ReplayedSeq,
}

/// 编辑器宿主入站类型白名单(封闭、带版本)。`selftest_done` 仅 spike 使用。
const EDITOR_INBOUND: &[&str] = &[
    "ready",
    "failed",
    "edit",
    "selection",
    "title",
    "snapshot",
    "closing",
    "resolve",
    "selftest_done",
    "selftest_report",
];

pub struct Validator {
    binding: Binding,
    last_seq: u64,
    max_bytes: usize,
}

impl Validator {
    pub fn new(binding: Binding, max_bytes: usize) -> Self {
        Self { binding, last_seq: 0, max_bytes }
    }

    pub fn accept(&mut self, raw: &str) -> Result<Envelope, Reject> {
        if raw.len() > self.max_bytes {
            return Err(Reject::TooLarge);
        }
        let env: Envelope = serde_json::from_str(raw).map_err(|_| Reject::Malformed)?;
        if env.host_kind != "editor" {
            return Err(Reject::WrongHostKind);
        }
        if env.binding != self.binding {
            return Err(Reject::ForeignBinding);
        }
        if !EDITOR_INBOUND.contains(&env.ty.as_str()) {
            return Err(Reject::UnknownType);
        }
        if env.seq <= self.last_seq {
            return Err(Reject::ReplayedSeq);
        }
        self.last_seq = env.seq;
        Ok(env)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(doc: &str) -> Binding {
        Binding { project_id: "p".into(), panel_id: "pn".into(), document_id: doc.into() }
    }
    fn msg(binding: &Binding, seq: u64, ty: &str, host: &str) -> String {
        serde_json::json!({"hostKind": host, "binding": binding, "seq": seq, "type": ty, "payload": {}}).to_string()
    }

    #[test]
    fn accepts_valid_and_requires_increasing_seq() {
        let mut v = Validator::new(b("d1"), 1024);
        assert!(v.accept(&msg(&b("d1"), 1, "ready", "editor")).is_ok());
        assert_eq!(v.accept(&msg(&b("d1"), 1, "edit", "editor")).unwrap_err(), Reject::ReplayedSeq);
        assert_eq!(v.accept(&msg(&b("d1"), 0, "edit", "editor")).unwrap_err(), Reject::ReplayedSeq);
        assert!(v.accept(&msg(&b("d1"), 2, "edit", "editor")).is_ok());
    }

    #[test]
    fn rejects_foreign_binding_without_advancing_seq() {
        let mut v = Validator::new(b("d1"), 1024);
        assert_eq!(v.accept(&msg(&b("other"), 5, "edit", "editor")).unwrap_err(), Reject::ForeignBinding);
        // 被拒消息不能推进 seq
        assert!(v.accept(&msg(&b("d1"), 1, "edit", "editor")).is_ok());
    }

    #[test]
    fn rejects_unknown_type_wrong_kind_oversize_and_malformed() {
        let mut v = Validator::new(b("d1"), 256);
        assert_eq!(v.accept(&msg(&b("d1"), 1, "exec_shell", "editor")).unwrap_err(), Reject::UnknownType);
        assert_eq!(v.accept(&msg(&b("d1"), 1, "edit", "preview")).unwrap_err(), Reject::WrongHostKind);
        assert_eq!(v.accept("not json").unwrap_err(), Reject::Malformed);
        assert_eq!(v.accept(&"x".repeat(257)).unwrap_err(), Reject::TooLarge);
        assert_eq!(v.accept(r#"{"hostKind":"editor"}"#).unwrap_err(), Reject::Malformed);
    }
}
