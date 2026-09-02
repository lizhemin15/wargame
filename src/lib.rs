//! wargame —— Rust 核心 + Lua 插件化兵棋引擎（M1 原型）
//!
//! 目标（架构定稿 v1.0）：
//!   - 确定性事件溯源（Rust 内核，字节级可回放）
//!   - 规则即插件（走法/裁判/兵种全 Lua，热插拔，内核零改动）
//!   - 轻量单文件部署
//!
//! 里程碑 M1 验证两命题：
//!   ① Lua 插件热插拔（改规则不重启内核）
//!   ② Rust 事件溯源字节级确定性回放

pub mod board;
pub mod hex;
pub mod engine;
pub mod event;
pub mod host;
pub mod combat;
pub mod move_rules;
pub mod ruleset;
pub mod snapshot;
pub mod web;

pub use board::Board;
pub use engine::{Engine, Outcome};
pub use event::{Command, Event};