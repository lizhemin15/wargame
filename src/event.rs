//! 事件溯源核心：命令 → 事件 → 状态折叠
//!
//! 确定性铁律：
//! 1. 裁决核心纯同步（命令 → 事件），无 IO/随机/wall-clock
//! 2. 事件 append-only，不可变，是单一事实源
//! 3. 状态由事件折叠而来，replay(events) 字节级还原
//!
//! M2.1 扩展：攻击/夺点/胜负。攻击结果在裁决时用规则**纯计算**
//! （攻击力 vs 防御力+地形，无掷骰），写进事件；replay 重放同结果保证确定性。

use serde::{Deserialize, Serialize};

/// 棋盘坐标（扁平索引，row*cols+col）
pub type Cell = u8;

/// 单位 ID
pub type UnitId = u8;

/// 要点 ID（胜利目标 grid 位置）
pub type PointId = usize;

/// 单位在棋盘上的位置状态
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unit {
    pub id: UnitId,
    pub kind: String, // 兵种名，对应 ruleset units 键
    pub cell: Cell,
    pub owner: u8,
    /// 生命值。>0 存活；规则裁决可减至 0 触发 Eliminated。
    #[serde(default = "default_hp")]
    pub hp: u32,
}

fn default_hp() -> u32 {
    1
}

/// —— 命令（玩家/引擎输入，进裁决流水线）——
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Command {
    Move {
        unit: UnitId,
        to: Cell,
    },
    /// attack 发起方 unit 攻击 target 单位（相邻或射程内）
    Attack {
        unit: UnitId,
        target: UnitId,
    },
}

/// —— 事件（裁决通过后固化，append-only 不可变）——
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Event {
    /// 移动成功
    MoveAccepted {
        unit: UnitId,
        from: Cell,
        to: Cell,
    },
    /// 攻击发起，附裁决结果（纯计算，确定性）
    AttackResolved {
        attacker: UnitId,
        defender: UnitId,
        defender_cell: Cell,
        /// 攻击是否消灭了守方
        hit: bool,
    },
    /// 单位被消灭（从棋盘移除）
    Eliminated {
        unit: UnitId,
        cell: Cell,
    },
    /// 要点被一方占领
    PointTaken {
        point: PointId,
        owner: u8,
    },
    /// 对局结束，宣布胜者
    GameOver {
        winner: u8,
        reason: String,
    },
}