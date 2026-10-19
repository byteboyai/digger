//! Digger 的宿主适配层:把产品特有语义注入共享面板库(Task 20)。
//!
//! 与 `dozer/crates/dozer-app/src/host_adapters/` 同型:共享库(`dozer-panel-usage`)
//! 只定义窄接缝,产品侧提供实现。这里有 Usage 与 Project 两个面板的适配。

pub mod files;
pub mod project;
pub mod usage;

pub mod usage_native;
