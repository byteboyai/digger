//! 桥的 Rust 一侧(ADR-013 §4–§5):收到页面发来的原始 JSON,回答要发回页面的消息。
//!
//! 纯库,不碰 webview:宿主适配层只负责把字节送进来、把回复送回页面,所以握手、nonce、自检门控、
//! 过期 base、未知消息这些规则都能在没有窗口的测试里验证。

use crate::doc::{AgentError, AgentOp, Doc, Step};
use crate::protocol::{
    Change, EditorInbound, EditorOutbound, Envelope, ErrorCode, PROTOCOL_VERSION,
};

/// 运行模式。`Agent` / `Load` / 结果上报只在自检模式被接受;正常运行时页面不能按块 id 改写文档。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    SelfTest,
}

/// 自检页上报的内容(正常运行时永远不会出现)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Report {
    Ready {
        engine: String,
    },
    Result {
        name: String,
        pass: bool,
        detail: String,
    },
    Info {
        name: String,
        detail: String,
    },
    Done,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Handled {
    /// 要发回页面的消息。
    Reply(EditorOutbound),
    /// 页面确认一次 flush 屏障已排空。
    Flushed {
        request_id: u64,
        version: u64,
        error: Option<String>,
    },
    /// 自检上报,宿主打印并据此退出。
    Report(Report),
}

/// 宿主(iced 栏的按钮)驱动的 Agent 替身:不经过页面,页面也不能触发它(ADR-013 §5)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StandIn {
    /// 改写第 n 个段落(n 超出就取模,方便连点)
    RewriteParagraph(usize),
    Append,
}

pub struct Bridge {
    doc: Doc,
    mode: Mode,
    nonce: String,
    /// 替身 Agent 已经做过的步数,写进它的文本里,让每次改写看得出区别
    stand_in_runs: usize,
    /// 最近一次带 request ID 的提交 `(id, 内容指纹, 原回包)`:重复请求返回原结果,不再次应用。
    last_request: Option<(u64, u64, EditorOutbound)>,
    /// 已见过的最大 request ID:更旧的 ID 无法判断是否重复,拒绝而不猜测。
    highest_request: u64,
}

fn fingerprint(raw: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    raw.hash(&mut h);
    h.finish()
}

enum Gate {
    Proceed,
    Replay(EditorOutbound),
    Reject(ErrorCode),
}

impl Bridge {
    /// `initial` 是 Rust 持有的初始文本;`nonce` 由宿主随机生成、写进页面的初始化脚本。
    pub fn new(initial: &str, mode: Mode, nonce: impl Into<String>) -> Self {
        let mut doc = Doc::new();
        doc.load(initial);
        Self {
            doc,
            mode,
            nonce: nonce.into(),
            stand_in_runs: 0,
            last_request: None,
            highest_request: 0,
        }
    }

    /// 同一 request ID 只应用一次;重复内容返回原回包,不同内容报协议错误。
    fn gate(&self, id: Option<u64>, fp: u64) -> Gate {
        let Some(id) = id else {
            return Gate::Proceed;
        };
        if let Some((last_id, last_fp, reply)) = &self.last_request
            && *last_id == id
        {
            return if *last_fp == fp {
                Gate::Replay(reply.clone())
            } else {
                Gate::Reject(ErrorCode::DuplicateRequest)
            };
        }
        if id < self.highest_request {
            return Gate::Reject(ErrorCode::OutOfOrderRequest);
        }
        Gate::Proceed
    }

    fn remember(&mut self, id: Option<u64>, fp: u64, reply: &EditorOutbound) {
        let Some(id) = id else {
            return;
        };
        self.highest_request = self.highest_request.max(id);
        self.last_request = Some((id, fp, reply.clone()));
    }

    pub fn doc(&self) -> &Doc {
        &self.doc
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// 保存快照的所有权:取走 pending history,由调用方在提交失败时通过
    /// [`Bridge::restore_history`] 归还。正文与块身份来自同一个 `State`。
    pub fn export_snapshot(&mut self) -> crate::doc::State {
        self.doc.export_state()
    }

    /// 保存失败时把未确认的 history 放回文档,避免下一次快照丢行。
    pub fn restore_history(&mut self, rows: Vec<(u64, u64, u64)>) {
        self.doc.restore_history(rows);
    }

    /// 切换文稿:用已保存的块身份重建 Doc(或从纯文本开始)。同时清空请求门,
    /// 旧 request id 不能跨文稿复用。
    pub fn replace_document(&mut self, state: Option<&crate::doc::State>, text: &str) {
        self.doc = match state {
            Some(state) => Doc::import_state(state, text),
            None => {
                let mut doc = Doc::new();
                doc.load(text);
                doc
            }
        };
        self.last_request = None;
        self.highest_request = 0;
    }

    /// 让替身 Agent 改文档,返回要推给页面的 `update`。没有可改的块或步骤失败时返回 `None`(文档不变)。
    pub fn stand_in(&mut self, what: StandIn) -> Option<EditorOutbound> {
        self.stand_in_runs += 1;
        let text = format!(
            "Agent 改写 #{}：这一段是 Agent 在你写作时改写的，用来检查输入法组合和撤销是否正常😀。",
            self.stand_in_runs
        );
        let op = match what {
            StandIn::Append => AgentOp::Append { text },
            StandIn::RewriteParagraph(n) => {
                let EditorOutbound::Blocks { blocks, .. } = self.doc.blocks_message() else {
                    return None;
                };
                let paragraphs: Vec<_> = blocks.iter().filter(|b| b.kind == "paragraph").collect();
                if paragraphs.is_empty() {
                    return None;
                }
                AgentOp::ReplaceBlock {
                    id: paragraphs[n % paragraphs.len()].id.clone(),
                    text,
                }
            }
        };
        let mut patches = self.doc.agent_plan(&[Step { op, expect: None }]).ok()?;
        Some(self.doc.update_message(&patches.remove(0), "agent"))
    }

    fn reject(
        &self,
        code: ErrorCode,
        request_id: Option<u64>,
        reason: impl Into<String>,
    ) -> Handled {
        Handled::Reply(EditorOutbound::Reject {
            code,
            request_id,
            reason: reason.into(),
            version: Some(self.doc.version),
        })
    }

    /// 处理一条上行消息。每条消息都有回答:不会有静默丢弃。
    pub fn handle(&mut self, raw: &str) -> Handled {
        let Envelope { n, msg } = match serde_json::from_str::<Envelope>(raw) {
            Ok(env) => env,
            Err(e) => {
                return self.reject(
                    ErrorCode::MalformedChanges,
                    None,
                    format!("unparsable message: {e}"),
                );
            }
        };
        if n != self.nonce {
            return self.reject(ErrorCode::Unauthorized, None, "bad nonce");
        }
        if msg.selftest_only() && self.mode != Mode::SelfTest {
            return self.reject(
                ErrorCode::Unauthorized,
                None,
                "not accepted outside selftest",
            );
        }
        match msg {
            EditorInbound::Ready { protocol, engine } => {
                if protocol.unwrap_or(1) != PROTOCOL_VERSION {
                    return self.reject(
                        ErrorCode::UnsupportedProtocol,
                        None,
                        format!(
                            "protocol {} is not supported (this build speaks {PROTOCOL_VERSION})",
                            protocol.unwrap_or(1)
                        ),
                    );
                }
                match self.mode {
                    Mode::Normal => Handled::Reply(self.doc.open_message()),
                    Mode::SelfTest => Handled::Report(Report::Ready {
                        engine: engine.unwrap_or_default(),
                    }),
                }
            }
            EditorInbound::Edit {
                request_id,
                base,
                changes,
            } => {
                let fp = fingerprint(raw);
                match self.gate(request_id, fp) {
                    Gate::Replay(reply) => return Handled::Reply(reply),
                    Gate::Reject(code) => {
                        return self.reject(
                            code,
                            request_id,
                            match code {
                                ErrorCode::DuplicateRequest => {
                                    "request id reused with different content"
                                }
                                _ => "request id is older than the last confirmed request",
                            },
                        );
                    }
                    Gate::Proceed => {}
                }
                let changes: Vec<(usize, usize, String)> = changes
                    .into_iter()
                    .map(|Change { from, to, insert }| (from, to, insert))
                    .collect();
                match self.doc.edit(base, &changes) {
                    Ok(()) => {
                        let reply = self.doc.after_edit_message(request_id);
                        self.remember(request_id, fp, &reply);
                        Handled::Reply(reply)
                    }
                    Err(e) => {
                        let code = match e {
                            crate::doc::EditError::StaleBase { .. } => ErrorCode::StaleBase,
                            crate::doc::EditError::BadPosition(_) => ErrorCode::BadPosition,
                            crate::doc::EditError::Malformed(_) => ErrorCode::MalformedChanges,
                        };
                        let reply = EditorOutbound::Reject {
                            code,
                            request_id,
                            reason: format!("{e:?}"),
                            version: Some(self.doc.version),
                        };
                        // 重复投递同一请求时返回同一拒绝,而不是再次尝试应用。
                        self.remember(request_id, fp, &reply);
                        Handled::Reply(reply)
                    }
                }
            }
            EditorInbound::Flushed {
                request_id,
                version,
                error,
            } => Handled::Flushed {
                request_id,
                version,
                error,
            },
            EditorInbound::Snapshot { request_id } => {
                let fp = fingerprint(raw);
                match self.gate(request_id, fp) {
                    Gate::Replay(reply) => return Handled::Reply(reply),
                    Gate::Reject(code) => {
                        return self.reject(code, request_id, "stale snapshot request id");
                    }
                    Gate::Proceed => {}
                }
                let reply = self.doc.snapshot_message(request_id);
                self.remember(request_id, fp, &reply);
                Handled::Reply(reply)
            }
            EditorInbound::Load { text } => {
                self.doc.load(&text);
                Handled::Reply(self.doc.blocks_message())
            }
            EditorInbound::Agent {
                op,
                id,
                text,
                expect,
            } => {
                let op = match op.as_str() {
                    "replace_block" => Some(AgentOp::ReplaceBlock { id, text }),
                    "insert_after" => Some(AgentOp::InsertAfter { id, text }),
                    "append" => Some(AgentOp::Append { text }),
                    _ => None,
                };
                Handled::Reply(
                    match op.map(|op| self.doc.agent_plan(&[Step { op, expect }])) {
                        Some(Ok(mut patches)) => {
                            self.doc.update_message(&patches.remove(0), "agent")
                        }
                        Some(Err((_, e))) => EditorOutbound::AgentError {
                            reason: e.to_string(),
                            stale: matches!(e, AgentError::Stale { .. }),
                        },
                        None => EditorOutbound::AgentError {
                            reason: "unknown operation".into(),
                            stale: false,
                        },
                    },
                )
            }
            EditorInbound::Result { name, pass, detail } => {
                Handled::Report(Report::Result { name, pass, detail })
            }
            EditorInbound::Info { name, detail } => Handled::Report(Report::Info { name, detail }),
            EditorInbound::Done {} => Handled::Report(Report::Done),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const N: &str = "nonce-1";

    fn msg(v: serde_json::Value) -> String {
        let mut v = v;
        v["n"] = json!(N);
        v.to_string()
    }

    fn normal() -> Bridge {
        Bridge::new("# 标题\n\n第一段。\n\n第二段。\n", Mode::Normal, N)
    }

    fn reply(h: Handled) -> EditorOutbound {
        match h {
            Handled::Reply(r) => r,
            other => panic!("expected a reply, got {other:?}"),
        }
    }

    fn reason(h: Handled) -> String {
        match reply(h) {
            EditorOutbound::Reject { reason, .. } => reason,
            other => panic!("expected reject, got {other:?}"),
        }
    }

    #[test]
    fn ready_is_answered_with_the_whole_document() {
        let mut b = normal();
        match reply(b.handle(&msg(json!({"t": "ready", "protocol": 2})))) {
            EditorOutbound::Open {
                text,
                version,
                blocks,
                len16,
                hash,
                ..
            } => {
                assert_eq!(text, "# 标题\n\n第一段。\n\n第二段。\n");
                assert_eq!(blocks.len(), 3);
                assert_eq!(version, b.doc().version);
                assert_eq!(len16, text.encode_utf16().count());
                assert_eq!(hash, crate::doc::fnv(&text));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn an_edit_is_applied_and_answered_with_blocks() {
        let mut b = normal();
        let v = b.doc().version;
        let r = reply(b.handle(&msg(
            json!({"t": "edit", "base": v, "changes": [{"from": 0, "to": 0, "insert": "x"}]}),
        )));
        assert!(matches!(r, EditorOutbound::Blocks { version, .. } if version == v + 1));
        assert!(b.doc().text.starts_with("x# 标题"));
    }

    #[test]
    fn a_stale_base_is_refused_with_the_current_version() {
        let mut b = normal();
        let v = b.doc().version;
        let h = b.handle(&msg(json!({"t": "edit", "base": v - 1, "changes": []})));
        match reply(h) {
            EditorOutbound::Reject {
                reason, version, ..
            } => {
                assert!(reason.contains("StaleBase"), "{reason}");
                assert_eq!(version, Some(v));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn nothing_is_dropped_silently() {
        let mut b = normal();
        // 不是 JSON / 孤立代理项 / 未知标签 / 缺字段:都有回答
        assert!(reason(b.handle("not json")).contains("unparsable"));
        assert!(reason(b.handle(&format!(r#"{{"n":"{N}","t":"edit","base":1,"changes":[{{"from":0,"to":0,"insert":"\ud800"}}]}}"#))).contains("unparsable"));
        assert!(reason(b.handle(&msg(json!({"t": "frobnicate"})))).contains("unparsable"));
        assert!(reason(b.handle(&msg(json!({"t": "edit"})))).contains("unparsable"));
    }

    #[test]
    fn a_message_without_or_with_the_wrong_nonce_is_refused() {
        let mut b = normal();
        let v = b.doc().version;
        assert!(
            reason(b.handle(&json!({"t": "ready", "protocol": 2}).to_string()))
                .contains("unparsable")
        );
        assert_eq!(reason(b.handle(&json!({"n": "other", "t": "edit", "base": v, "changes": [{"from": 0, "to": 0, "insert": "x"}]}).to_string())), "bad nonce");
        assert_eq!(b.doc().version, v, "a refused message changes nothing");
    }

    #[test]
    fn the_agent_hook_and_the_harness_reports_are_refused_in_normal_operation() {
        let mut b = normal();
        let before = b.doc().text.clone();
        let id = match reply(b.handle(&msg(json!({"t": "ready", "protocol": 2})))) {
            EditorOutbound::Open { blocks, .. } => blocks[1].id.clone(),
            other => panic!("{other:?}"),
        };
        for m in [
            json!({"t": "agent", "op": "replace_block", "id": id, "text": "pwned"}),
            json!({"t": "agent", "op": "append", "text": "pwned"}),
            json!({"t": "load", "text": "pwned"}),
            json!({"t": "result", "name": "x", "pass": true}),
            json!({"t": "info", "name": "x"}),
            json!({"t": "done"}),
        ] {
            assert_eq!(
                reason(b.handle(&msg(m.clone()))),
                "not accepted outside selftest",
                "{m}"
            );
        }
        assert_eq!(
            b.doc().text,
            before,
            "a webview must not be able to rewrite blocks by id"
        );
    }

    #[test]
    fn the_host_can_drive_the_agent_stand_in_in_normal_operation() {
        let mut b = normal();
        let v = b.doc().version;
        let up = b
            .stand_in(StandIn::RewriteParagraph(1))
            .expect("a paragraph exists");
        assert!(
            matches!(up, EditorOutbound::Update { ref origin, version, .. } if origin == "agent" && version == v + 1),
            "{up:?}"
        );
        assert!(
            b.doc().text.contains("第一段。") && !b.doc().text.contains("第二段。"),
            "{:?}",
            b.doc().text
        );
        assert!(b.stand_in(StandIn::Append).is_some());
        assert!(b.doc().text.trim_end().ends_with("😀。"));
        let mut empty = Bridge::new("# 只有标题\n", Mode::Normal, N);
        assert!(
            empty.stand_in(StandIn::RewriteParagraph(0)).is_none(),
            "no paragraph, nothing to rewrite"
        );
        assert_eq!(empty.doc().text, "# 只有标题\n");
    }

    #[test]
    fn selftest_mode_accepts_the_agent_hook_and_reports() {
        let mut b = Bridge::new("甲\n\n乙\n", Mode::SelfTest, N);
        let id = b.doc().blocks_message();
        let EditorOutbound::Blocks { blocks, .. } = id else {
            panic!()
        };
        let r = reply(b.handle(&msg(
            json!({"t": "agent", "op": "replace_block", "id": blocks[1].id, "text": "丙"}),
        )));
        assert!(
            matches!(r, EditorOutbound::Update { ref origin, .. } if origin == "agent"),
            "{r:?}"
        );
        assert_eq!(b.doc().text, "甲\n\n丙\n");
        let stale = reply(b.handle(&msg(json!({"t": "agent", "op": "replace_block", "id": blocks[0].id, "text": "x", "expect": "0000000000000000"}))));
        assert!(
            matches!(stale, EditorOutbound::AgentError { stale: true, .. }),
            "{stale:?}"
        );
        assert!(matches!(
            reply(b.handle(&msg(json!({"t": "agent", "op": "nope"})))),
            EditorOutbound::AgentError { stale: false, .. }
        ));
        assert_eq!(
            b.handle(&msg(
                json!({"t": "result", "name": "T1", "pass": false, "detail": "d"})
            )),
            Handled::Report(Report::Result {
                name: "T1".into(),
                pass: false,
                detail: "d".into()
            })
        );
        assert_eq!(
            b.handle(&msg(json!({"t": "done"}))),
            Handled::Report(Report::Done)
        );
        assert_eq!(
            b.handle(&msg(json!({"t": "ready", "protocol": 2, "engine": "cm"}))),
            Handled::Report(Report::Ready {
                engine: "cm".into()
            })
        );
    }

    #[test]
    fn request_ids_are_applied_once_and_duplicates_are_answered_from_cache() {
        let mut b = normal();
        let v = b.doc().version;
        let first = msg(json!({"t": "edit", "request_id": 7, "base": v,
            "changes": [{"from": 0, "to": 0, "insert": "x"}]}));
        match reply(b.handle(&first)) {
            EditorOutbound::Blocks {
                request_id,
                version,
                ..
            } => {
                assert_eq!((request_id, version), (Some(7), v + 1));
            }
            other => panic!("{other:?}"),
        }
        let text = b.doc().text.clone();
        // Same id and content: replay the original result, do not apply the change twice.
        match reply(b.handle(&first)) {
            EditorOutbound::Blocks {
                request_id,
                version,
                ..
            } => {
                assert_eq!((request_id, version), (Some(7), v + 1));
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(b.doc().text, text, "the edit is not applied twice");
        assert_eq!(b.doc().version, v + 1);
    }

    #[test]
    fn the_same_request_id_with_different_content_is_a_protocol_error() {
        let mut b = normal();
        let v = b.doc().version;
        let first = msg(json!({"t": "edit", "request_id": 3, "base": v,
            "changes": [{"from": 0, "to": 0, "insert": "x"}]}));
        assert!(matches!(
            reply(b.handle(&first)),
            EditorOutbound::Blocks { .. }
        ));
        let text = b.doc().text.clone();
        let different = msg(json!({"t": "edit", "request_id": 3, "base": v + 1,
            "changes": [{"from": 0, "to": 0, "insert": "y"}]}));
        match reply(b.handle(&different)) {
            EditorOutbound::Reject {
                code, request_id, ..
            } => {
                assert_eq!(code, ErrorCode::DuplicateRequest);
                assert_eq!(request_id, Some(3));
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(b.doc().text, text);
    }

    #[test]
    fn an_older_request_id_is_refused_without_guessing() {
        let mut b = normal();
        let v = b.doc().version;
        for (id, base) in [(5, v), (6, v + 1)] {
            let m = msg(json!({"t": "edit", "request_id": id, "base": base,
                "changes": [{"from": 0, "to": 0, "insert": "x"}]}));
            assert!(matches!(reply(b.handle(&m)), EditorOutbound::Blocks { .. }));
        }
        let text = b.doc().text.clone();
        let old = msg(json!({"t": "edit", "request_id": 5, "base": v + 2,
            "changes": [{"from": 0, "to": 0, "insert": "z"}]}));
        match reply(b.handle(&old)) {
            EditorOutbound::Reject { code, .. } => {
                assert_eq!(code, ErrorCode::OutOfOrderRequest);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(b.doc().text, text);
    }

    #[test]
    fn a_read_only_snapshot_is_allowed_in_normal_mode_and_does_not_change_the_document() {
        let mut b = normal();
        b.handle(&msg(json!({"t": "edit", "request_id": 1, "base": 1,
            "changes": [{"from": 0, "to": 0, "insert": "x"}]})));
        let (text, version) = (b.doc().text.clone(), b.doc().version);
        match reply(b.handle(&msg(json!({"t": "snapshot", "request_id": 2})))) {
            EditorOutbound::Snapshot {
                request_id,
                text: snapshot,
                version: got_version,
                ..
            } => {
                assert_eq!(request_id, Some(2));
                assert_eq!(snapshot, text);
                assert_eq!(got_version, version);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!((b.doc().text.clone(), b.doc().version), (text, version));
    }

    #[test]
    fn an_incompatible_protocol_stops_the_handshake() {
        let mut b = normal();
        match reply(b.handle(&msg(json!({"t": "ready", "protocol": 1})))) {
            EditorOutbound::Reject { code, .. } => {
                assert_eq!(code, ErrorCode::UnsupportedProtocol);
            }
            other => panic!("{other:?}"),
        }
        // A page that omits the version is an old page, not a new one.
        match reply(b.handle(&msg(json!({"t": "ready"})))) {
            EditorOutbound::Reject { code, .. } => {
                assert_eq!(code, ErrorCode::UnsupportedProtocol);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn wire_format_is_the_one_the_spike_validated() {
        let mut b = normal();
        let json = reply(b.handle(&msg(json!({"t": "ready", "protocol": 2})))).to_json();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["t"], "open");
        assert_eq!(v["protocol"], 2);
        for k in ["text", "version", "hash", "len16", "blocks"] {
            assert!(v.get(k).is_some(), "open.{k}");
        }
        for k in ["id", "kind", "level", "title", "how", "from", "to", "head"] {
            assert!(v["blocks"][0].get(k).is_some(), "block.{k}");
        }
        let r = EditorOutbound::AgentError {
            reason: "r".into(),
            stale: true,
        }
        .to_json();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&r).unwrap(),
            json!({"t": "agent_error", "reason": "r", "stale": true})
        );
    }
}
