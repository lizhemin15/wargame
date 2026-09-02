//! 六边形网格几何 —— 轴向/立方体坐标转换与相邻判定。
//!
//! 内部用立方体坐标 (x, y, z) 满足 x+y+z=0（redblobgames 命名）。
//! 存储与前端仍用 (row, col) 偏移坐标（pointy-top, odd-r：奇数行右移半格）。
//!
//! 六邻方向（立方体）：
//!   (+1,-1,0) (-1,+1,0)  (+1,0,-1) (-1,0,+1)  (0,+1,-1) (0,-1,+1)
//!   —— 三轴 × 两向，直线判据：差向量恰有一个坐标分量为 0。

/// 立方体坐标
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cube {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

const DIRS: [Cube; 6] = [
    Cube { x: 1, y: -1, z: 0 },
    Cube { x: 1, y: 0, z: -1 },
    Cube { x: 0, y: 1, z: -1 },
    Cube { x: -1, y: 1, z: 0 },
    Cube { x: -1, y: 0, z: 1 },
    Cube { x: 0, y: -1, z: 1 },
];

/// odd-r 偏移坐标 (row, col) → 立方体。奇数行右移半格。
pub fn offset_to_cube(row: i32, col: i32) -> Cube {
    let x = col - (row - (row & 1)) / 2;
    let z = row;
    let y = -x - z;
    Cube { x, y, z }
}

/// 立方体 → odd-r 偏移坐标 (row, col)
pub fn cube_to_offset(c: Cube) -> (i32, i32) {
    let row = c.z;
    let col = c.x + (c.z - (c.z & 1)) / 2;
    (row, col)
}

/// 立方体距离（六边形曼哈顿）
pub fn distance(a: Cube, b: Cube) -> i32 {
    (a.x - b.x).abs().max((a.y - b.y).abs()).max((a.z - b.z).abs())
}

/// (row,col) 六邻域，已在界内。返回 (row,col) 列表。
pub fn neighbors(row: i32, col: i32, rows: usize, cols: usize) -> Vec<(usize, usize)> {
    let c = offset_to_cube(row, col);
    let mut out = Vec::with_capacity(6);
    for d in DIRS {
        let n = Cube { x: c.x + d.x, y: c.y + d.y, z: c.z + d.z };
        let (r, cc) = cube_to_offset(n);
        if r >= 0 && cc >= 0 && (r as usize) < rows && (cc as usize) < cols {
            out.push((r as usize, cc as usize));
        }
    }
    out
}

/// 两格是否在同一条六边形直线（三轴之一）上。cube 差向量恰有一个分量为 0。
pub fn same_line(fr: usize, fc: usize, tr: usize, tc: usize) -> bool {
    let a = offset_to_cube(fr as i32, fc as i32);
    let b = offset_to_cube(tr as i32, tc as i32);
    let (dx, dy, dz) = (b.x - a.x, b.y - a.y, b.z - a.z);
    dx == 0 || dy == 0 || dz == 0
}

/// from→to 沿网格线（三轴之一）逐格推进，返回中间格（不含 from，含 to）。
/// 两格必须在同一直线上，否则返回 None。
pub fn line_cells(fr: usize, fc: usize, tr: usize, tc: usize, rows: usize, cols: usize) -> Option<Vec<(usize, usize)>> {
    if fr == tr && fc == tc {
        return None;
    }
    if !same_line(fr, fc, tr, tc) {
        return None;
    }
    let a = offset_to_cube(fr as i32, fc as i32);
    let b = offset_to_cube(tr as i32, tc as i32);
    let n = distance(a, b);
    // 步长在三个坐标轴的符号（每一步沿差向量单位方向）
    let (sx, sy, sz) = ((b.x - a.x).signum(), (b.y - a.y).signum(), (b.z - a.z).signum());
    let mut out = Vec::with_capacity(n as usize);
    let mut c = a;
    for _ in 0..n {
        c = Cube { x: c.x + sx, y: c.y + sy, z: c.z + sz };
        let (r, cc) = cube_to_offset(c);
        if r < 0 || cc < 0 || (r as usize) >= rows || (cc as usize) >= cols {
            return None; // 越界视为路径中断
        }
        out.push((r as usize, cc as usize));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(r: i32, col: i32) -> Cube {
        offset_to_cube(r, col)
    }

    fn dist(r1: i32, c1: i32, r2: i32, c2: i32) -> i32 {
        distance(c(r1, c1), c(r2, c2))
    }

    #[test]
    fn offset_roundtrip() {
        for (r, col) in [(0i32, 0i32), (0, 5), (1, 3), (2, 4), (7, 0), (14, 20)] {
            let cb = offset_to_cube(r, col);
            assert_eq!(cube_to_offset(cb), (r, col), "roundtrip ({r},{col})");
        }
    }

    #[test]
    fn six_neighbors_have_dist_one() {
        // 内部格：偶行(r=2)与奇行(r=3)+最小内部(1,1)的邻域都应距离1
        for center in [(2i32, 5i32), (3i32, 5i32), (1, 1)] {
            let (cr, cc) = center;
            let ns = neighbors(cr, cc, 15, 21);
            assert_eq!(ns.len(), 6, "内部格应有6个邻居 @({cr},{cc})，实际 {:?}", ns);
            for (nr, nc) in ns {
                assert_eq!(dist(cr, cc, nr as i32, nc as i32), 1, "邻居距离应=1");
            }
        }
        // 边界格邻居数更少（(0,0) 为角格只有2个）
        assert!(neighbors(0, 0, 3, 3).len() < 6);
        assert_eq!(neighbors(0, 0, 3, 3).len(), 2);
    }

    #[test]
    fn same_line_matches_distance() {
        // 同行 (0,0)->(0,2) 在 hex 中为直线（轴向），距离2
        assert!(same_line(0, 0, 0, 2), "(0,0)->(0,2) 应共线");
        // 距离：同行 (0,0)->(0,2) = 2
        // (0,0)->(2,2) 不共线（非三轴之一）
        assert!(!same_line(0, 0, 2, 2), "(0,0)->(2,2) 非 hex 直线");
    }

    #[test]
    fn line_cells_between() {
        let cells = line_cells(0, 0, 0, 2, 3, 3).expect("在线上");
        // 同行线性：应含中间(0,1) + 终点，逐格推进
        assert_eq!(cells.len(), 2);
        let (mr, mc) = cells[0];
        assert_eq!((mr, mc), (0, 1), "第一步应到同行相邻格");
        assert_eq!(dist(0, 0, mr as i32, mc as i32), 1, "第一步到相邻格");
        assert_eq!(dist(mr as i32, mc as i32, 0, 2), 1, "最后一步到终点");
    }
}