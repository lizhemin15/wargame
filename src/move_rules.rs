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
    let style_ok = match rs.grid {
        crate::ruleset::Grid::Hex => hex_style_ok(ut, fr, fc, tr, tc, rows, cols, rs),
        crate::ruleset::Grid::Square => square_style_ok(ut, fr, fc, tr, tc, rs),
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

/// 方形网格几何校验（原逻辑）—— leap/step 用 move_offsets 偏移表，slide 十字直线。
fn square_style_ok(ut: &UnitType, fr: usize, fc: usize, tr: usize, tc: usize, rs: &Ruleset) -> bool {
    let (dr, dc) = (tr as i16 - fr as i16, tc as i16 - fc as i16);
    match ut.move_style {
        MoveStyle::Leap => ut
            .move_offsets
            .iter()
            .any(|(or, oc)| *or as i16 == dr && *oc as i16 == dc),
        MoveStyle::Step => {
            // 方形单步：上下左右 4 邻
            let is_neighbor = (dr.abs() + dc.abs()) == 1;
            is_neighbor && ut
                .move_offsets
                .iter()
                .any(|(or, oc)| *or as i16 == dr && *oc as i16 == dc)
        }
        MoveStyle::Slide => {
            let is_cardinal = (dr != 0 && dc == 0) || (dr == 0 && dc != 0);
            is_cardinal && slide_path_clear(fr, fc, tr, tc, rs)
        }
    }
}

/// 六边形网格几何校验—— step=6邻域，slide=三轴直线，leap=按 hex 距离正好落在 offsets 距。
fn hex_style_ok(
    ut: &UnitType,
    fr: usize,
    fc: usize,
    tr: usize,
    tc: usize,
    rows: usize,
    cols: usize,
    rs: &Ruleset,
) -> bool {
    let is_neighbor = crate::hex::neighbors(fr as i32, fc as i32, rows, cols)
        .iter()
        .any(|&(r, c)| r == tr && c == tc);
    match ut.move_style {
        MoveStyle::Leap => {
            // hex leap：须沿三轴之一、且 hex 距离命中某条 move_offsets 的长度。
            let d = crate::hex::distance(
                crate::hex::offset_to_cube(fr as i32, fc as i32),
                crate::hex::offset_to_cube(tr as i32, tc as i32),
            );
            let d_i32 = d as i32;
            crate::hex::same_line(fr, fc, tr, tc)
                && ut
                    .move_offsets
                    .iter()
                    .any(|(or, oc)| (or.abs().max(oc.abs())) as i32 == d_i32)
        }
        MoveStyle::Step => is_neighbor,
        MoveStyle::Slide => {
            if !crate::hex::same_line(fr, fc, tr, tc) {
                return false;
            }
            match crate::hex::line_cells(fr, fc, tr, tc, rows, cols) {
                Some(cells) => cells
                    .iter()
                    // 途经格必须可通行；终点格放行（终点可通行性在 can_move 单独校验）
                    .all(|&(r, c)| (r == tr && c == tc) || rs.passable(r, c)),
                None => false,
            }
        }
    }
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

    /// hex 版 mini —— 同 3x3，但 grid=hex，测 6 邻 / 三轴。
    const MINI_HEX: &str = r#"
name = "mini_hex"
grid = "hex"
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
[units.warrior]
name = "步兵"
class = "infantry"
move_points = 1
move_style = "step"
move_offsets = [[1,0],[-1,0],[0,1],[0,-1]]
[units.tank]
name = "战车"
class = "mounted"
move_points = 5
move_style = "slide"
move_offsets = [[1,0],[-1,0],[0,1],[0,-1]]

[terrain]
rows = 3
cols = 3
cells = [".",".",".", ".","~",".", ".",".","."]

[[deploy]]
kind = "warrior"
row = 0
col = 0
owner = 0
"#;

    fn mini_hex() -> Ruleset {
        Ruleset::from_toml(MINI_HEX).expect("mini_hex ruleset")
    }

    #[test]
    fn hex_step_six_neighbor() {
        let rs = mini_hex();
        let b = Board::from_ruleset(&rs);
        let ut = rs.units.get("warrior").unwrap();
        // hex 中心 (1,1) 是水；步兵 step 从(0,0)。(0,1) 是 (0,0) 的 hex 邻居 → 合法（平原）
        let v = can_move(&rs, ut, b.cell_at(0, 0), b.cell_at(0, 1), b.rows, b.cols, &b.occ);
        assert!(v.is_ok(), "(0,0)->(0,1) 相邻应合法: {:?}", v);
        // (0,0) -> (2,2) 距离2 非单步 → 拒绝
        let v2 = can_move(&rs, ut, b.cell_at(0, 0), b.cell_at(2, 2), b.rows, b.cols, &b.occ);
        assert!(
            matches!(v2, MoveVerdict::Rejected(_)),
            "(0,0)->(2,2) 非相邻应拒绝: {:?}",
            v2
        );
    }

    #[test]
    fn hex_slide_three_axis() {
        let rs = mini_hex();
        let b = Board::from_ruleset(&rs);
        let ut = rs.units.get("tank").unwrap();
        // (0,0)->(2,2) 在 hex 三轴上，中间(1,1)是水 → 阻挡拒绝
        let v = can_move(&rs, ut, b.cell_at(0, 0), b.cell_at(2, 2), b.rows, b.cols, &b.occ);
        assert_eq!(
            v,
            MoveVerdict::Rejected("兵种移动方式不允许该走法".into()),
            "hex slide 穿水应阻挡"
        );
        // (0,0)->(1,2) 非三轴直线 → 拒绝
        let v2 = can_move(&rs, ut, b.cell_at(0, 0), b.cell_at(1, 2), b.rows, b.cols, &b.occ);
        assert!(
            matches!(v2, MoveVerdict::Rejected(_)),
            "非直线滑行应拒绝: {:?}",
            v2
        );
        // 回归：单步 slide（终点即唯一途经格）不得被终点放行逻辑误拒
        let v3 = can_move(&rs, ut, b.cell_at(0, 0), b.cell_at(0, 1), b.rows, b.cols, &b.occ);
        assert!(v3.is_ok(), "单步 hex slide (0,0)->(0,1) 应合法: {:?}", v3);
        // 回归：两格清空直线 slide（途经格非终点）应合法
        let v4 = can_move(&rs, ut, b.cell_at(0, 0), b.cell_at(0, 2), b.rows, b.cols, &b.occ);
        assert!(v4.is_ok(), "两格直线 slide (0,0)->(0,2) 应合法: {:?}", v4);
    }
}