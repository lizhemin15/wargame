//! M2.1 攻击/夺点/胜负判定（纯计算，确定性无掷骰）
//!
//! 确定性铁律：攻防结果由规则**纯计算**得出，不依赖随机数/墙钟。
//!   - 攻击命中: 攻方 attack vs 守方 defense + 守方地形防御加成
//!   - 射程: range 格内（曼哈顿距离）
//!   - 夺点: 单位进入无守军的要点格 → 自动占领（在 engine 裁决）
//!   - 胜负: 达到 victory.need_points 占领数，或消灭指挥官 → GameOver

use crate::board::Board;
use crate::event::UnitId;
use crate::ruleset::Ruleset;

/// 由地形符号（单字符）取地形定义（用于防御加成/通行）
fn terrain_of(rs: &Ruleset, symbol: char) -> Option<&crate::ruleset::Terrain> {
    rs.terrains.values().find(|t| t.symbol == symbol)
}

/// 攻击是否命中（消灭守方）。纯确定性计算。
pub fn attack_hits(
    rs: &Ruleset,
    attacker_kind: &str,
    defender: &crate::event::Unit,
    board: &Board,
) -> bool {
    let a = match rs.units.get(attacker_kind) {
        Some(u) => u,
        None => return false,
    };
    let d = match rs.units.get(&defender.kind) {
        Some(u) => u,
        None => return false,
    };

    // 守方有效防御 = 基础防御 + 地形防御加成（守方所在格地形）
    let terrain_bonus = terrain_of(rs, board.terrain_symbol(defender.cell))
        .map(|t| t.defense_bonus)
        .unwrap_or(0);
    let def_total = d.defense.saturating_add(terrain_bonus);

    // 攻方有效攻击 = 基础攻击。可加地形/背水加成（M2.1b）。
    let atk_total = a.attack;

    atk_total >= def_total
}

/// 攻击射程内？（曼哈顿距离 ≤ range）
pub fn in_range(
    rs: &Ruleset,
    attacker_kind: &str,
    from: crate::event::Cell,
    to: crate::event::Cell,
    board: &Board,
) -> bool {
    let range = rs
        .units
        .get(attacker_kind)
        .map(|u| u.range)
        .unwrap_or(1) as i32;
    let (af, ac) = board.cell_pos(from);
    let (tf, tc) = board.cell_pos(to);
    let dr = (af as i32 - tf as i32).abs();
    let dc = (ac as i32 - tc as i32).abs();
    let dist = dr + dc;
    dist > 0 && dist <= range
}

/// 胜负判定：某方占领要点数达 need_points，或消灭指挥官。
/// 返回 Some(胜者 owner)。
pub fn check_victory(rs: &Ruleset, board: &Board) -> Option<u8> {
    // 1) 占领要点数达标
    if let Some(v) = &rs.victory {
        let need = v.need_points;
        if need > 0 {
            let counts = board.objective_owners(rs);
            for (&owner, &n) in &counts {
                if n >= need {
                    return Some(owner);
                }
            }
        }
        // 2) 指挥官被消灭 → 对方胜利
        for cmd in &v.commanders {
            // 找该指挥官单位的 owner；若该单位已死亡(hp==0)，对手胜
            let found = board
                .units
                .iter()
                .find(|(_, u)| u.kind == *cmd);
            if let Some((_, u)) = found {
                if !board.is_alive(u.id) {
                    // 指挥官死了 → 另一阵营胜（owner 0↔1 互换）
                    return Some(1 - u.owner);
                }
            }
        }
    }
    None
}

/// 便捷：曼哈顿距离
pub fn manhattan(a: (u8, u8), b: (u8, u8)) -> i32 {
    (a.0 as i32 - b.0 as i32).abs() + (a.1 as i32 - b.1 as i32).abs()
}

/// 地形防御加成查询（供测试/debug）
pub fn terrain_defense(rs: &Ruleset, symbol: char) -> u32 {
    terrain_of(rs, symbol).map(|t| t.defense_bonus).unwrap_or(0)
}

#[allow(dead_code)]
fn unused(_u: UnitId) {}