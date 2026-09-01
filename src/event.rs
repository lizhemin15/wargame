//! 事件溯源核心：命令 → 事件 → 状态折叠
//!
//! 确定性铁律：
//! 1. 裁决核心纯同步（命令 → 事件），无 IO/随机/wall-clock
//! 2. 事件 append-only，不可变，是单一事实源
//! 3. 状态由事件折叠而来，replay(events) 字节级还原

use serde::{Deserialize, Serialize};

/// 棋盘坐标（扁平索引，8x8 = 0..64）
pub type Cell = u8;

/// 单位 ID（数字，马兵种用）
pub type UnitId = u8;

/// 单位在棋盘上的位置状态
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unit {
    pub id: UnitId,
    pub kind: String, // 兵种名，对应 Lua 插件名
    pub cell: Cell,
    pub owner: u8,
}

/// —— 命令（玩家/引擎输入，进裁决流水线）——
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Command {
    Move {
        unit: UnitId,
        to: Cell,
    },
}

/// —— 事件（裁决通过后固化，append-only 不可变）——
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Event {
    MoveAccepted {
        unit: UnitId,
        from: Cell,
        to: Cell,
    },
}