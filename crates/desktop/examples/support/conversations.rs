//! Synthetic-only data source shared by the gallery and fixture contract tests.
use dozer_model::{AgentKind, ConversationSummary, TurnRecord};
use dozer_services::{
    BoxFuture, ConversationRows, ConversationService, ServiceError, ServiceErrorKind,
};
use std::sync::Arc;

pub const FIXTURES: [&str; 7] = [
    "short", "tools", "long", "huge", "hostile", "empty", "broken",
];
pub const STATES: [&str; 10] = [
    "empty",
    "list-only",
    "list-and-detail-loading",
    "list-and-detail",
    "detail-error",
    "detail-empty-entries",
    "long-detail",
    "import-prompt",
    "daemon-unavailable",
    "agent-filter-open",
];

pub fn fixture(name: &str) -> String {
    std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/conversations")
            .join(format!("{name}.jsonl")),
    )
    .unwrap()
}
pub fn turns(name: &str, text: &str) -> Vec<TurnRecord> {
    dozerd::transcripts::parse::parse_chunk(AgentKind::Claude, text, name, 0)
        .into_iter()
        .enumerate()
        .map(|(index, turn)| {
            let detail = dozerd::transcripts::parse::extract_turn_trace_detail(
                &turn.raw_json,
                AgentKind::Claude,
            );
            TurnRecord {
                turn_index: index as i64,
                role: turn.role,
                content: turn.content,
                thinking: turn.thinking,
                thinking_text: detail.thinking_text,
                tool_calls: detail.tool_calls,
                tool_result_call_id: detail.tool_result_call_id,
                ts: turn.ts,
                is_error: turn.is_error,
                tokens_in: turn.tokens_in,
                tokens_out: turn.tokens_out,
                tokens_cache_read: turn.tokens_cache_read,
                tokens_cache_write: turn.tokens_cache_write,
            }
        })
        .collect()
}

pub struct FakeConversationService {
    pub state: String,
}
impl ConversationService for FakeConversationService {
    fn list_conversations_with_summaries(
        &self,
        _: String,
        _: Option<AgentKind>,
        _: u32,
        _: u32,
    ) -> BoxFuture<'_, Result<ConversationRows, ServiceError>> {
        let result = if self.state == "daemon-unavailable" {
            Err(ServiceError::new(
                ServiceErrorKind::Unavailable,
                "Synthetic daemon unavailable (no connection attempted)",
            ))
        } else if matches!(self.state.as_str(), "empty" | "import-prompt") {
            Ok(vec![])
        } else {
            Ok(FIXTURES
                .iter()
                .map(|name| {
                    (
                        ConversationSummary {
                            conversation_id: (*name).into(),
                            agent: AgentKind::Claude,
                            file_path: format!("/synthetic/{name}.jsonl"),
                            title: format!("Synthetic {name}"),
                            first_ts: 1792324800000,
                            last_ts: 1792324800000,
                            turn_count: match *name {
                                "long" => 1000,
                                "huge" => 10000,
                                "empty" => 0,
                                "tools" => 5,
                                "broken" | "hostile" => 2,
                                _ => 6,
                            },
                        },
                        None,
                    )
                })
                .collect())
        };
        Box::pin(async move { result })
    }
    fn get_conversation_turns(
        &self,
        cid: String,
        after: i64,
        limit: u32,
    ) -> BoxFuture<'_, Result<Vec<TurnRecord>, ServiceError>> {
        Box::pin(async move {
            if self.state == "detail-error" {
                return Err(ServiceError::new(
                    ServiceErrorKind::Unavailable,
                    "Synthetic detail failure",
                ));
            }
            if !FIXTURES.contains(&cid.as_str()) {
                return Err(ServiceError::new(
                    ServiceErrorKind::Unavailable,
                    "Unknown synthetic fixture",
                ));
            }
            Ok(turns(&cid, &fixture(&cid))
                .into_iter()
                .filter(|t| t.turn_index > after)
                .take(limit as usize)
                .collect())
        })
    }
}
#[derive(Default)]
pub struct DisabledServices;
fn disabled<T>() -> Result<T, ServiceError> {
    Err(ServiceError::new(
        ServiceErrorKind::Unavailable,
        "Synthetic gallery: operation disabled",
    ))
}
impl dozer_services::SummaryService for DisabledServices {
    fn submit_summary_job(&self, _: String, _: bool) -> BoxFuture<'_, Result<i64, ServiceError>> {
        Box::pin(async { disabled() })
    }
    fn get_summary_job(
        &self,
        _: i64,
    ) -> BoxFuture<'_, Result<Option<dozer_model::SummaryJobInfo>, ServiceError>> {
        Box::pin(async { disabled() })
    }
}
impl dozer_services::TranscriptService for DisabledServices {
    fn delete_project_transcripts(&self, _: String) -> BoxFuture<'_, Result<u32, ServiceError>> {
        Box::pin(async { disabled() })
    }
    fn backfill_project_transcripts(&self, _: String) -> BoxFuture<'_, Result<u32, ServiceError>> {
        Box::pin(async { disabled() })
    }
    fn submit_summary_batch(
        &self,
        _: String,
        _: Option<AgentKind>,
        _: Option<String>,
    ) -> BoxFuture<'_, Result<(i64, u32), ServiceError>> {
        Box::pin(async { disabled() })
    }
    fn get_summary_batch(
        &self,
        _: i64,
    ) -> BoxFuture<'_, Result<Option<dozer_model::SummaryBatchInfo>, ServiceError>> {
        Box::pin(async { disabled() })
    }
}
pub fn unavailable() -> Arc<DisabledServices> {
    Arc::new(DisabledServices)
}
