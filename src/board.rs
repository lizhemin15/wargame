//! 棋盘状态（事件溯源的可折叠状态）
//! 状态只由 apply() 折叠事件而来，是纯函数，无 IO/随机。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::event::{Cell, Event, Unit, UnitId};

/// 棋盘尺寸 8x8
pub const SIZE: usize = 8;
pub const CELLS: usize = SIZE * SIZE;

/// 棋盘状态：单位按 id 存，另建 cell→unit 索引
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Board {
    /// unit_id -> unit
    pub units: BTreeMap<UnitId, Unit>,
    /// cell -> unit_id（占位索引）
    pub occ: BTreeMap<Cell, UnitId>,
}

impl Board {
    /// 初始布局（回到开局）
    pub fn initial() -> Self {
        let mut b = Board::default();
        // 演示：红马在 e4 (格4)，黑马在 f6 (格45)
        b.place(Unit { id: 1, kind: "knight".into(), cell: 4, owner: 0 });
        b.place(Unit { id: 2, kind: "knight".into(), cell: 45, owner: 1 });
        b
    }

    /// 放置一个单位（初始化和载入用）
    pub fn place(&mut self, u: Unit) {
        let cell = u.cell;
        let id = u.id;
        self.occ.remove(&cell);
        self.units.insert(id, u);
        self.occ.insert(cell, id);
    }

    pub fn get(&self, id: UnitId) -> Option<&Unit> {
        self.units.get(&id)
    }

    pub fn at(&self, cell: Cell) -> Option<UnitId> {
        self.occ.get(&cell).copied()
    }

    /// —— 事件折叠：唯一改状态的地方，纯函数 ——
    pub fn apply(&mut self, ev: &Event) {
        match ev {
            Event::MoveAccepted { unit, from, to } => {
                // 从棋盘移除旧位
                self.occ.remove(from);
                // 更新单位
                if let Some(u) = self.units.get_mut(unit) {
                    u.cell = *to;
                }
                // 落到新位（允许吃子：覆盖存在者）
                self.occ.insert(*to, *unit);
            }
        }
    }

    /// 完整序列化为规范 JSON（确定性）。
    /// 绕 BTreeMap 保证字段序稳定 → 序列化结果字节级一致 → 可做 golden hash
    pub fn to_canonical_json(&self) -> String {
        serde_json::to_string(self).expect("board is serializable")
    }

    /// 展平为单位列表（渲染/测试用）
    pub fn to_units_vec(&self) -> Vec<(Cell, &Unit)> {
        self.units.values().map(|u| (u.cell, u)).collect()
    }

    /// 字节级确定性比较两个棋盘状态
    pub fn equivalent(&self, other: &Board) -> bool {
        self.to_canonical_json() == other.to_canonical_json()
    }
}