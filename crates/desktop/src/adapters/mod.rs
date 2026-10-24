//! Digger 的宿主适配层:把产品特有语义注入共享面板库(Task 20)。
//!
//! 与 `dozer/crates/dozer-app/src/host_adapters/` 同型:共享库(`dozer-panel-usage`)
//! 只定义窄接缝,产品侧提供实现。这里有 Usage 与 Project 两个面板的适配。

pub mod agent;
pub mod agent_hooks;
pub mod agent_ime;
// 键位转换由 S6d-2 的终端输入接线消费(S6d-2 Task 5 待输入法探针结果)。
pub mod agent_display;
#[allow(dead_code)]
pub mod agent_keys;
pub mod agent_layout;
pub mod agent_tabs;
pub mod files;
pub mod menu_overlay;
pub mod project;
pub mod usage;

pub mod usage_native;

/// 预览原生宿主(S5b-2 P3):清单 / 协议 / 注入脚本 / IPC 全走共享库。
pub mod preview_native;

pub mod preview;
pub mod preview_bridge;

/// 预览页签栏部件(页签 / 溢出入口 / 下拉 / 模式按钮)。
pub mod tab_chrome;

/// 预览面板的 `PanelDisplay<Msg>` 实现(库视图反过来要的展示能力)。
pub mod preview_display;
