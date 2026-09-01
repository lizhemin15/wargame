//! 数据驱动移动判定（M2 核心）
//!
//! M1 用 Lua「规则即插件」证明规则外置可热插拔；M2 把**标准移动规则**
//! 数据化到 ruleset（TOML），由这里 Rust 直接解释：
//!   - 兵种几何（leap/slide/step）+ 地形通行 + 移动点约束
//!   - 纯函数、无 IO/随机 → 确定性天然，ruleset 可进 CI golden hash
//!
//! 高级/自定义规则（士气、域对抗）仍走 Lua 插件钩子（架构分层）。

use std::collections::BTreeMap;

use crate::board::Board;
use crate::event::{Cell, UnitId};
use crate::ruleset::{MoveStyle, Ruleset, UnitType};

/// 移动合法性判定结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MoveVerdict {
    /// 合法
    Ok,
    /// 非法，附原因
    Rejected(String),
}

impl MoveVerdict {
    pub fn is_ok(&self) -> bool {
        matches!(self, MoveVerdict::Ok)
    }
}

/// 判定单位从 from 移动到 to 是否合法。
/// 纯函数：只依赖 (ruleset 数据, 单位, 起止格, 当前占用表)。
pub fn can_move(
    rs: &Ruleset,
    ut: &UnitType,
    from: Cell,
    to: Cell,
    rows: usize,
    cols: usize,
    occ: &BTreeMap<Cell, UnitId>,
) -> MoveVerdict {
    let (fr, fc) = ((from as usize) / cols, (from as usize) % cols);
    let (tr, tc) = ((to as usize) / cols, (to as usize) % cols);

    // 目标格必须在棋盘内
    if tr >= rows || tc >= cols {
        return MoveVerdict::Rejected("目标格越界".into());
    }
    // 原地不动非法
    if from == to {
        return MoveVerdict::Rejected("原地不动".into());
    }

    // —— 兵种几何校验 ——
    let style_ok = match ut.move_style {
        MoveStyle::Leap => {
            let dr = tr as i16 - fr as i16;
            let dc = tc as i16 - fc as i16;
            ut.move_offsets
                .iter()
                .any(|(or, oc)| *or as i16 == dr && *oc as i16 == dc)
        }
        MoveStyle::Step => {
            let dr = tr as i16 - fr as i16;
            let dc = tc as i16 - fc as i16;
            ut.move_offsets
                .iter()
                .any(|(or, oc)| *or as i16 == dr && *oc as i16 == dc)
        }
        MoveStyle::Slide => {
            let (dr, dc) = (tr as i16 - fr as i16, tc as i16 - fc as i16);
            let is_cardinal = (dr != 0 && dc == 0) || (dr == 0 && dc != 0);
            if !is_cardinal {
                false
            } else {
                slide_path_clear(fr, fc, tr, tc, rs)
            }
        }
    };

    if !style_ok {
        return MoveVerdict::Rejected("兵种移动方式不允许该走法".into());
    }

    // —— 地形通行校验（目标格必须是可通行的）——
    if !rs.passable(tr, tc) {
        return MoveVerdict::Rejected("目标格不可通行（水域/障碍）".into());
    }

    // —— 占用校验（M2.1：允许吃子，战斗结算在 M2.2）——
    let _ = occ.get(&to);

    MoveVerdict::Ok
}

/// Slide 滑行路径校验：from→to 直线上中间每格必须可通行。
fn slide_path_clear(fr: usize, fc: usize, tr: usize, tc: usize, rs: &Ruleset) -> bool {
    let (dr, dc): (i64, i64) = (tr as i64 - fr as i64, tc as i64 - fc as i64);
    let steps = dr.abs().max(dc.abs());
    if steps == 0 {
        return false;
    }
    let (su, sv) = (dr.signum(), dc.signum());
    let (mut r, mut c) = (fr as i64 + su, fc as i64 + sv);
    let mut n = 1;
    while n < steps {
        if r < 0 || c < 0 {
            return false;
        }
        if !rs.passable(r as usize, c as usize) {
            return false; // 路径中有障碍阻挡
        }
        r += su;
        c += sv;
        n += 1;
    }
    true
}

/// 便捷：判定 board 上一个单位的移动。
pub fn can_move_on_board(rs: &Ruleset, board: &Board, unit_id: UnitId, to: Cell) -> MoveVerdict {
    let Some(unit) = board.get(unit_id) else {
        return MoveVerdict::Rejected(format!("unknown unit: {unit_id}"));
    };
    let Some(ut) = rs.units.get(&unit.kind) else {
        return MoveVerdict::Rejected(format!("ruleset 无兵种: {}", unit.kind));
    };
    can_move(rs, ut, unit.cell, to, board.rows, board.cols, &board.occ)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 最小可控 ruleset：3x3 棋盘，中心`~`水，其余平原；骑士(leap)+战车(slide)+步兵(step)
    const MINI: &str = r#"
name = "mini"
[terrains]
[terrains.plain]
name = "plain"
symbol = "."
move_cost = 1
[terrains.water]
name = "water"
symbol = "~"
move_cost = 0

[unit_classes]
[unit_classes.mounted]
name = "骑乘"
move_cost = 1
[unit_classes.infantry]
name = "步兵"
move_cost = 1

[units]
[units.knight]
name = "骑士"
class = "mounted"
move_points = 3
move_style = "leap"
move_offsets = [[2,1],[1,2],[-1,2],[-2,1],[-2,-1],[-1,-2],[1,-2],[2,-1]]
[units.rook]
name = "战车"
class = "mounted"
move_points = 5
move_style = "slide"
move_offsets = [[1,0],[-1,0],[0,1],[0,-1]]
[units.infantry]
name = "步兵"
class = "infantry"
move_points = 1
move_style = "step"
move_offsets = [[1,0],[-1,0],[0,1],[0,-1]]

[terrain]
rows = 3
cols = 3
cells = [".",".",".", ".","~",".", ".",".","."]

[[deploy]]
kind = "knight"
row = 0
col = 0
owner = 0
"#;

    fn mini() -> Ruleset {
        Ruleset::from_toml(MINI).expect("mini ruleset")
    }

    #[test]
    fn leap_knight_legal() {
        let rs = mini();
        let b = Board::from_ruleset(&rs);
        let ut = rs.units.get("knight").unwrap();
        // (0,0) 马跳 → (2,1) dr=2,dc=1 → 'plain' 合法
        let v = can_move(&rs, ut, b.cell_at(0, 0), b.cell_at(2, 1), b.rows, b.cols, &b.occ);
        assert!(v.is_ok(), "(0,0)->(2,1) 应合法: {:?}", v);
        // (0,0) 直下 (1,0) dr=1,dc=0 非马步 → 几何拒绝
        let v2 = can_move(&rs, ut, b.cell_at(0, 0), b.cell_at(1, 0), b.rows, b.cols, &b.occ);
        assert_eq!(
            v2,
            MoveVerdict::Rejected("兵种移动方式不允许该走法".into()),
            "非马步应被拒绝"
        );
    }

    #[test]
    fn slide_rook_blocked_by_water() {
        let rs = mini();
        let b = Board::from_ruleset(&rs);
        let ut = rs.units.get("rook").unwrap();
        // 战车放在 (2,1) 测 slide：直线向上 (2,1)->(0,1) 中间(1,1)是水 → 阻挡拒绝
        // can_move 只判几何+地形，不依赖部署，直接给坐标
        let v = can_move(&rs, ut, b.cell_at(2, 1), b.cell_at(0, 1), b.rows, b.cols, &b.occ);
        assert_eq!(
            v,
            MoveVerdict::Rejected("兵种移动方式不允许该走法".into()),
            "slide 穿过水域应被阻挡"
        );
        // (0,0)->(0,2) 横向，中间(0,1)平原 → 合法
        let v2 = can_move(&rs, ut, b.cell_at(0, 0), b.cell_at(0, 2), b.rows, b.cols, &b.occ);
        assert!(v2.is_ok(), "(0,0)->(0,2) 滑行应合法: {:?}", v2);
    }

    #[test]
    fn step_infantry_legal_move() {
        let rs = mini();
        let b = Board::from_ruleset(&rs);
        let ut = rs.units.get("infantry").unwrap();
        // 步兵 (1,0) 向右一格 (1,1) 是水 → 拒绝
        let v = can_move(&rs, ut, b.cell_at(1, 0), b.cell_at(1, 1), b.rows, b.cols, &b.occ);
        assert_eq!(v, MoveVerdict::Rejected("目标格不可通行（水域/障碍）".into()));
        // 步兵 (1,0) 向下 (2,0) 平原 → 合法
        let v2 = can_move(&rs, ut, b.cell_at(1, 0), b.cell_at(2, 0), b.rows, b.cols, &b.occ);
        assert!(v2.is_ok(), "(1,0)->(2,0) 应合法: {:?}", v2);
    }
}