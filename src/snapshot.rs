//! 稳定快照（snapshot）—— 可视化插件看到的"投影"。
//!
//! 方案A：插件不直接绑定 Board / Unit / Ruleset 的内部字段，
//! 而是读取一个**固定的、语义稳定**的快照 schema。
//! Rust 侧 `snapshot()` 负责把当前内部结构翻译成这个 schema；
//! 内部结构怎么加字段、改字段，插件代码零改动（由翻译层吸收）。
//!
//! 这里定义的是 schema 的 Rust 结构 + 从 Engine 构建快照的翻译函数。

use std::collections::BTreeMap;

use mlua::{Lua, Table};

use crate::board::Board;
use crate::event::{Event, PointId, Unit};
use crate::ruleset::Ruleset;

/// 单个单位在快照中的投影（字段固定，不跟 Unit 内部走）
#[derive(Debug, Clone)]
pub struct SnapUnit {
    pub id: u16,
    pub kind: String,
    pub owner: i32,
    pub cell: i32, // 扁平索引；-1 = 已消灭
    pub hp: i32,
}

/// 单个要点在快照中的投影
#[derive(Debug, Clone)]
pub struct SnapPoint {
    pub id: i32,
    pub name: String,
    pub cell: i32,
    pub owner: i32, // -1 = 未占领
}

/// 快照本体：稳定 schema，插件只认这一层
pub struct Snapshot {
    pub ruleset_name: String,
    pub rows: i32,
    pub cols: i32,
    pub terrain: Vec<String>, // 扁平符号数组，长度 rows*cols
    pub units: Vec<SnapUnit>,
    pub points: Vec<SnapPoint>,
    pub winner: i32,  // -1 = 未分胜负
    pub logs: Vec<String>,
}

/// 从 Engine 的 board + ruleset 构建快照（翻译层核心）。
/// 注意：不直接暴露 Unit/Ruleset 内部，只输出稳定 schema。
pub fn build(board: &Board, ruleset: &Ruleset, logs: &[Event], winner: Option<u8>) -> Snapshot {
    let mut units = Vec::new();
    for u in board.units.values() {
        if u.hp == 0 {
            continue; // 已消灭的不进快照
        }
        units.push(snap_unit(u));
    }
    // 按 cell 排序，方便插件画图
    units.sort_by_key(|u| u.cell);

    let mut points = Vec::new();
    for (i, obj) in ruleset.objectives.iter().enumerate() {
        let cell = board.cell_at(obj.row as usize, obj.col as usize) as i32;
        let owner = board
            .point_owner
            .get(&i)
            .copied()
            .map(|o| o as i32)
            .unwrap_or(-1);
        points.push(SnapPoint {
            id: i as i32,
            name: obj.name.clone(),
            cell,
            owner,
        });
    }

    let logs_str = logs
        .iter()
        .map(|e| serde_json::to_string(e).unwrap_or_default())
        .collect();

    Snapshot {
        ruleset_name: ruleset.name.clone(),
        rows: ruleset.terrain.rows as i32,
        cols: ruleset.terrain.cols as i32,
        terrain: board.terrain.clone(),
        units,
        points,
        winner: winner.map(|w| w as i32).unwrap_or(-1),
        logs: logs_str,
    }
}

fn snap_unit(u: &Unit) -> SnapUnit {
    SnapUnit {
        id: u.id as u16,
        kind: u.kind.clone(),
        owner: u.owner as i32,
        cell: u.cell as i32,
        hp: u.hp as i32,
    }
}

/// 把快照压成 Lua 表，注入给渲染插件（当前性能下每次重建，规模小）。
pub fn to_lua(lua: &Lua, snap: &Snapshot, unit_display: &dyn Fn(&str) -> String) -> Table {
    let t = lua
        .create_table_with_capacity(0, 8)
        .expect("lua table");

    let _ = t.set("ruleset_name", snap.ruleset_name.clone());
    let _ = t.set("rows", snap.rows);
    let _ = t.set("cols", snap.cols);

    // terrain：扁平字符串数组（stable schema）
    let terrain = lua.create_table().expect("terrain");
    for (i, ts) in snap.terrain.iter().enumerate() {
        let _ = terrain.set(i + 1, ts.clone()); // 1-indexed
    }
    let _ = t.set("terrain", terrain);

    // units
    let units_t = lua.create_table().expect("units");
    for (i, u) in snap.units.iter().enumerate() {
        let ut = lua.create_table().expect("unit");
        let _ = ut.set("id", u.id);
        let _ = ut.set("kind", u.kind.clone());
        let _ = ut.set("owner", u.owner);
        let _ = ut.set("cell", u.cell);
        let _ = ut.set("hp", u.hp);
        let _ = ut.set("display", unit_display(&u.kind));
        let _ = units_t.set(i + 1, ut);
    }
    let _ = t.set("units", units_t);

    // points
    let points_t = lua.create_table().expect("points");
    for (i, p) in snap.points.iter().enumerate() {
        let pt = lua.create_table().expect("point");
        let _ = pt.set("id", p.id);
        let _ = pt.set("name", p.name.clone());
        let _ = pt.set("cell", p.cell);
        let _ = pt.set("owner", p.owner);
        let _ = points_t.set(i + 1, pt);
    }
    let _ = t.set("points", points_t);

    let _ = t.set("winner", snap.winner);

    let logs_t = lua.create_table().expect("logs");
    for (i, lg) in snap.logs.iter().enumerate() {
        let _ = logs_t.set(i + 1, lg.clone());
    }
    let _ = t.set("logs", logs_t);

    t
}

/// 供 board.apply 用的：把 PointId 映射成要点数组下标（用于 owner 查询的辅助）。

#[allow(dead_code)]
pub fn point_owner(board: &Board) -> BTreeMap<PointId, u8> {
    board.point_owner.clone()
}