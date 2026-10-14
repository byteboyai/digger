//! 块解析与块 ID 对齐。模块边界与将来抽出的 `markdown-core` 一致(ADR-013 §2):抽出时是搬走,不是重写。

pub mod parse;
pub mod reconcile;
