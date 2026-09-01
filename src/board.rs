//! 棋盘状态（事件溯源的可折叠状态）
//! 状态只由 apply() 折叠事件而来，是纯函数，无 IO/随机。
//! M2：棋盘尺寸/地形/初始部署全部来自 ruleset（数据驱动）。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::event::{Cell, Event, Unit, UnitId};
use crate::ruleset::Ruleset;

/// 棋盘状态：单位按 id 存，另建 cell→unit 索引
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Board {
    /// 行数
    pub rows: usize,
    /// 列数
    pub cols: usize,
    /// unit_id -> unit
    pub units: BTreeMap<UnitId, Unit>,
    /// cell -> unit_id（占位索引）
    pub occ: BTreeMap<Cell, UnitId>,
}

impl Board {
    /// 从 ruleset 构建初始棋盘（布局来自 deploy 数据）
    pub fn from_ruleset(rs: &Ruleset) -> Board {
        let mut b = Board {
            rows: rs.terrain.rows,
            cols: rs.terrain.cols,
            units: BTreeMap::new(),
            occ: BTreeMap::new(),
        };
        for (i, d) in rs.deploy.iter().enumerate() {
            let cell = b.cell_at(d.row as usize, d.col as usize);
            b.place(Unit {
                id: (i + 1) as UnitId,
                kind: d.kind.clone(),
                cell,
                owner: d.owner,
            });
        }
        b
    }

    /// 空棋盘（测试/自定义用）
    pub fn empty(rows: usize, cols: usize) -> Board {
        Board {
            rows,
            cols,
            units: BTreeMap::new(),
            occ: BTreeMap::new(),
        }
    }

    /// (row, col) → 扁平 cell
    pub fn cell_at(&self, row: usize, col: usize) -> Cell {
        (row * self.cols + col) as Cell
    }

    /// 扁平 cell → (row, col)
    pub fn to_rc(&self, cell: Cell) -> (usize, usize) {
        ((cell as usize) / self.cols, (cell as usize) % self.cols)
    }

    /// cell 是否在棋盘内
    pub fn in_bounds(&self, row: usize, col: usize) -> bool {
        row < self.rows && col < self.cols
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