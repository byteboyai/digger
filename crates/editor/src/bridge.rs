//! 桥的 Rust 一侧(ADR-013 §4–§5):收到页面发来的原始 JSON,回答要发回页面的消息。
//!
//! 纯库,不碰 webview:宿主适配层只负责把字节送进来、把回复送回页面,所以握手、nonce、自检门控、
//! 过期 base、未知消息这些规则都能在没有窗口的测试里验证。

use crate::doc::{AgentError, AgentOp, Doc, Step};
use crate::protocol::{Change, EditorInbound, EditorOutbound, Envelope};

/// 运行模式。`Agent` / `Load` / 结果上报只在自检模式被接受;正常运行时页面不能按块 id 改写文档。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    SelfTest,
}

/// 自检页上报的内容(正常运行时永远不会出现)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Report {
    Ready { engine: String },
    Result { name: String, pass: bool, detail: String },
    Info { name: String, detail: String },
    Done,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Handled {
    /// 要发回页面的消息。
    Reply(EditorOutbound),
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
}

impl Bridge {
    /// `initial` 是 Rust 持有的初始文本;`nonce` 由宿主随机生成、写进页面的初始化脚本。
    pub fn new(initial: &str, mode: Mode, nonce: impl Into<String>) -> Self {
        let mut doc = Doc::new();
        doc.load(initial);
        Self { doc, mode, nonce: nonce.into(), stand_in_runs: 0 }
    }

    pub fn doc(&self) -> &Doc {
        &self.doc
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// 让替身 Agent 改文档,返回要推给页面的 `update`。没有可改的块或步骤失败时返回 `None`(文档不变)。
    pub fn stand_in(&mut self, what: StandIn) -> Option<EditorOutbound> {
        self.stand_in_runs += 1;
        let text = format!("Agent 改写 #{}：这一段是 Agent 在你写作时改写的，用来检查输入法组合和撤销是否正常😀。", self.stand_in_runs);
        let op = match what {
            StandIn::Append => AgentOp::Append { text },
            StandIn::RewriteParagraph(n) => {
                let EditorOutbound::Blocks { blocks, .. } = self.doc.blocks_message() else { return None };
                let paragraphs: Vec<_> = blocks.iter().filter(|b| b.kind == "paragraph").collect();
                if paragraphs.is_empty() {
                    return None;
                }
                AgentOp::ReplaceBlock { id: paragraphs[n % paragraphs.len()].id.clone(), text }
            }
        };
        let mut patches = self.doc.agent_plan(&[Step { op, expect: None }]).ok()?;
        Some(self.doc.update_message(&patches.remove(0), "agent"))
    }

    fn reject(&self, reason: impl Into<String>) -> Handled {
        Handled::Reply(EditorOutbound::Reject { reason: reason.into(), version: Some(self.doc.version) })
    }

    /// 处理一条上行消息。每条消息都有回答:不会有静默丢弃。
    pub fn handle(&mut self, raw: &str) -> Handled {
        let Envelope { n, msg } = match serde_json::from_str::<Envelope>(raw) {
            Ok(env) => env,
            Err(e) => return self.reject(format!("unparsable message: {e}")),
        };
        if n != self.nonce {
            return self.reject("bad nonce");
        }
        if msg.selftest_only() && self.mode != Mode::SelfTest {
            return self.reject("not accepted outside selftest");
        }
        match msg {
            EditorInbound::Ready { engine } => match self.mode {
                Mode::Normal => Handled::Reply(self.doc.open_message()),
                Mode::SelfTest => Handled::Report(Report::Ready { engine: engine.unwrap_or_default() }),
            },
            EditorInbound::Edit { base, changes } => {
                let changes: Vec<(usize, usize, String)> = changes.into_iter().map(|Change { from, to, insert }| (from, to, insert)).collect();
                match self.doc.edit(base, &changes) {
                    Ok(()) => Handled::Reply(self.doc.blocks_message()),
                    Err(e) => self.reject(format!("{e:?}")),
                }
            }
            EditorInbound::Load { text } => {
                self.doc.load(&text);
                Handled::Reply(self.doc.blocks_message())
            }
            EditorInbound::Agent { op, id, text, expect } => {
                let op = match op.as_str() {
                    "replace_block" => Some(AgentOp::ReplaceBlock { id, text }),
                    "insert_after" => Some(AgentOp::InsertAfter { id, text }),
                    "append" => Some(AgentOp::Append { text }),
                    _ => None,
                };
                Handled::Reply(match op.map(|op| self.doc.agent_plan(&[Step { op, expect }])) {
                    Some(Ok(mut patches)) => self.doc.update_message(&patches.remove(0), "agent"),
                    Some(Err((_, e))) => EditorOutbound::AgentError { reason: e.to_string(), stale: matches!(e, AgentError::Stale { .. }) },
                    None => EditorOutbound::AgentError { reason: "unknown operation".into(), stale: false },
                })
            }
            EditorInbound::Result { name, pass, detail } => Handled::Report(Report::Result { name, pass, detail }),
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
        match reply(b.handle(&msg(json!({"t": "ready"})))) {
            EditorOutbound::Open { text, version, blocks, len16, hash } => {
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
        let r = reply(b.handle(&msg(json!({"t": "edit", "base": v, "changes": [{"from": 0, "to": 0, "insert": "x"}]}))));
        assert!(matches!(r, EditorOutbound::Blocks { version, .. } if version == v + 1));
        assert!(b.doc().text.starts_with("x# 标题"));
    }

    #[test]
    fn a_stale_base_is_refused_with_the_current_version() {
        let mut b = normal();
        let v = b.doc().version;
        let h = b.handle(&msg(json!({"t": "edit", "base": v - 1, "changes": []})));
        match reply(h) {
            EditorOutbound::Reject { reason, version } => {
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
        assert!(reason(b.handle(&json!({"t": "ready"}).to_string())).contains("unparsable"));
        assert_eq!(reason(b.handle(&json!({"n": "other", "t": "edit", "base": v, "changes": [{"from": 0, "to": 0, "insert": "x"}]}).to_string())), "bad nonce");
        assert_eq!(b.doc().version, v, "a refused message changes nothing");
    }

    #[test]
    fn the_agent_hook_and_the_harness_reports_are_refused_in_normal_operation() {
        let mut b = normal();
        let before = b.doc().text.clone();
        let id = match reply(b.handle(&msg(json!({"t": "ready"})))) {
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
            assert_eq!(reason(b.handle(&msg(m.clone()))), "not accepted outside selftest", "{m}");
        }
        assert_eq!(b.doc().text, before, "a webview must not be able to rewrite blocks by id");
    }

    #[test]
    fn the_host_can_drive_the_agent_stand_in_in_normal_operation() {
        let mut b = normal();
        let v = b.doc().version;
        let up = b.stand_in(StandIn::RewriteParagraph(1)).expect("a paragraph exists");
        assert!(matches!(up, EditorOutbound::Update { ref origin, version, .. } if origin == "agent" && version == v + 1), "{up:?}");
        assert!(b.doc().text.contains("第一段。") && !b.doc().text.contains("第二段。"), "{:?}", b.doc().text);
        assert!(b.stand_in(StandIn::Append).is_some());
        assert!(b.doc().text.trim_end().ends_with("😀。"));
        let mut empty = Bridge::new("# 只有标题\n", Mode::Normal, N);
        assert!(empty.stand_in(StandIn::RewriteParagraph(0)).is_none(), "no paragraph, nothing to rewrite");
        assert_eq!(empty.doc().text, "# 只有标题\n");
    }

    #[test]
    fn selftest_mode_accepts_the_agent_hook_and_reports() {
        let mut b = Bridge::new("甲\n\n乙\n", Mode::SelfTest, N);
        let id = b.doc().blocks_message();
        let EditorOutbound::Blocks { blocks, .. } = id else { panic!() };
        let r = reply(b.handle(&msg(json!({"t": "agent", "op": "replace_block", "id": blocks[1].id, "text": "丙"}))));
        assert!(matches!(r, EditorOutbound::Update { ref origin, .. } if origin == "agent"), "{r:?}");
        assert_eq!(b.doc().text, "甲\n\n丙\n");
        let stale = reply(b.handle(&msg(json!({"t": "agent", "op": "replace_block", "id": blocks[0].id, "text": "x", "expect": "0000000000000000"}))));
        assert!(matches!(stale, EditorOutbound::AgentError { stale: true, .. }), "{stale:?}");
        assert!(matches!(reply(b.handle(&msg(json!({"t": "agent", "op": "nope"})))), EditorOutbound::AgentError { stale: false, .. }));
        assert_eq!(b.handle(&msg(json!({"t": "result", "name": "T1", "pass": false, "detail": "d"}))), Handled::Report(Report::Result { name: "T1".into(), pass: false, detail: "d".into() }));
        assert_eq!(b.handle(&msg(json!({"t": "done"}))), Handled::Report(Report::Done));
        assert_eq!(b.handle(&msg(json!({"t": "ready", "engine": "cm"}))), Handled::Report(Report::Ready { engine: "cm".into() }));
    }

    #[test]
    fn wire_format_is_the_one_the_spike_validated() {
        let mut b = normal();
        let json = reply(b.handle(&msg(json!({"t": "ready"})))).to_json();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["t"], "open");
        for k in ["text", "version", "hash", "len16", "blocks"] {
            assert!(v.get(k).is_some(), "open.{k}");
        }
        for k in ["id", "kind", "level", "title", "how", "from", "to", "head"] {
            assert!(v["blocks"][0].get(k).is_some(), "block.{k}");
        }
        let r = EditorOutbound::AgentError { reason: "r".into(), stale: true }.to_json();
        assert_eq!(serde_json::from_str::<serde_json::Value>(&r).unwrap(), json!({"t": "agent_error", "reason": "r", "stale": true}));
    }
}
