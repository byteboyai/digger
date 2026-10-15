//! Digger 任务完成用量的领域持久化(计划 Task 20,闸门 2)。
//!
//! 这里只做**业务事实**:任务跑完时把这一次的用量(token / 工具调用 / 改动文件 / 起止时刻)
//! 幂等地记进一个落盘账本,并能按项目路径 + 时间区间查回来喂给共享 Usage 面板
//! ([`dozer_services::UsageService`])。本 crate 属纯业务层(`digger-store`),
//! **不得**依赖任何 `bytehost-*` 宿主 crate——面板/宿主接线由 `digger-desktop` 注入(见规格 §4 所有权表)。
//!
//! 设计要点(见 `api-decisions.md` §14):
//!
//! - **唯一真相是落盘账本**:进程崩溃/被杀不丢已完成记录;重新载入即恢复。
//! - **幂等**:以任务运行 id 为键 upsert——同一个完成事件重复投递(重试/重放)不会翻倍。
//! - **原子落盘**:先写临时文件再 rename(`StoreCleanups` 同款),半截 JSON 不被当成有效账本。
//! - **单位/时区**:时刻一律 Unix 毫秒(UTC),不带本地时区;面板侧再按需格式化。
//! - **读失败 ≠ 空**:账本损坏是 `Err`,与"确实没有记录"(`Ok(vec![])`)区分开,不静默吞。

mod ledger;
mod projects;
mod runner;
mod service;

pub use projects::ProjectHost;
pub use ledger::{CompletionEvent, TaskUsageRecord, UsageLedger, UsageLedgerError};
pub use runner::{Clock, RunningTask, RunningTasks, SystemClock, TaskOutcome, TaskRunner};
pub use service::{LedgerUsageService, UnavailableUsageService};
