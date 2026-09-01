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
    /// 地形符号网格（来自 ruleset.terrain.cells，状态自包含）
    #[serde(default)]
    pub terrain: Vec<String>,
    /// 要点 -> 当前占领方（PointTaken 事件折叠而来）
    #[serde(default)]
    pub point_owner: BTreeMap<crate::event::PointId, u8>,
}

impl Board {
    /// 从 ruleset 构建初始棋盘（布局来自 deploy 数据）
    pub fn from_ruleset(rs: &Ruleset) -> Board {
        let mut b = Board {
            rows: rs.terrain.rows,
            cols: rs.terrain.cols,
            units: BTreeMap::new(),
            occ: BTreeMap::new(),
            terrain: rs.terrain.cells.clone(),
            point_owner: BTreeMap::new(),
        };
        for (i, d) in rs.deploy.iter().enumerate() {
            let cell = b.cell_at(d.row as usize, d.col as usize);
            b.place(Unit {
                id: (i + 1) as UnitId,
                kind: d.kind.clone(),
                cell,
                owner: d.owner,
                hp: 1,
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
            terrain: vec![String::new(); rows * cols],
            point_owner: BTreeMap::new(),
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

    /// (row, col) = cell 的坐标（cell_pos 别名，供 combat 用）
    pub fn cell_pos(&self, cell: Cell) -> (usize, usize) {
        self.to_rc(cell)
    }

    /// cell 处地形符号（单字符）
    pub fn terrain_symbol(&self, cell: Cell) -> char {
        self.terrain
            .get(cell as usize)
            .and_then(|s| s.chars().next())
            .unwrap_or('?')
    }

    /// 单位是否仍存活（在棋盘上）
    pub fn is_alive(&self, id: UnitId) -> bool {
        self.units.contains_key(&id)
    }

    /// 各单位在当前棋盘上是否存活（死单位也保留在 units 里，hp 标记）——用作统计
    pub fn unit_alive_iter(&self) -> impl Iterator<Item = &Unit> {
        self.units.values().filter(|u| u.hp > 0)
    }

    /// 统计各方占领的要点数（要点易手后，owner 状态由 occ 位置折叠得出）
    /// 传入 ruleset 以解析哪些 cell 是要点。返回 owner -> 占领要点数。
    pub fn objective_owners(&self, rs: &Ruleset) -> std::collections::HashMap<u8, u32> {
        let mut m: std::collections::HashMap<u8, u32> = std::collections::HashMap::new();
        for (_i, obj) in rs.objectives.iter().enumerate() {
            let cell = self.cell_at(obj.row as usize, obj.col as usize);
            if let Some(uid) = self.occ.get(&cell) {
                if let Some(u) = self.units.get(uid) {
                    if u.hp > 0 {
                        *m.entry(u.owner).or_insert(0) += 1;
                    }
                }
            }
        }
        m
    }

    /// —— 事件折叠：唯一改状态的地方，纯函数 ——
    pub fn apply(&mut self, ev: &Event) {
        match ev {
            Event::MoveAccepted { unit, from, to } => {
                // 从棋盘移除旧位
                self.occ.remove(from);
                // 更新单位（移走被吃单位：若目标格有别的单位，先移除覆盖）
                self.occ.remove(to);
                if let Some(u) = self.units.get_mut(unit) {
                    u.cell = *to;
                }
                // 落到新位
                self.occ.insert(*to, *unit);
            }
            Event::Eliminated { unit, cell } => {
                self.occ.remove(cell);
                if let Some(u) = self.units.get_mut(unit) {
                    u.hp = 0;
                }
            }
            Event::AttackResolved { defender, attacker: _, defender_cell, hit } => {
                // 参考：若无独立 Eliminated 事件，这里执行灭杀。本引擎裁决时会连发
                // AttackResolved(hit) 与 Eliminated，Eliminated 负责状态折叠，
                // 此处仅作透明记录（保持折叠唯一性，不重复改状态）。
                let _ = (defender, defender_cell, hit);
            }
            Event::PointTaken { point, owner } => {
                self.point_owner.insert(*point, *owner);
            }
            Event::GameOver { .. } => {
                // 终止标记，无状态变化
            }
        }
    }

    /// 完整序列化为规范 JSON（确定性）。
    /// 绕 BTreeMap 保证字段序稳定 → 序列化结果字节级一致 → 可做 golden hash
    pub fn to_canonical_json(&self) -> String {
        serde_json::to_string(self).expect("board is serializable")
    }

    /// 展平为单位列表（渲染/测试用）——过滤已死亡(hp==0)单位
    pub fn to_units_vec(&self) -> Vec<(Cell, &Unit)> {
        self.units
            .values()
            .filter(|u| u.hp > 0)
            .map(|u| (u.cell, u))
            .collect()
    }

    /// 字节级确定性比较两个棋盘状态
    pub fn equivalent(&self, other: &Board) -> bool {
        self.to_canonical_json() == other.to_canonical_json()
    }
}