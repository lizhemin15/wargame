//! 数据驱动规则集（Ruleset）—— M2 核心
//!
//! M1 用 Lua「规则即插件」证明了规则的外置可热插拔。
//! M2 把**标准规则**（兵种/地形/移动/初始部署）升级为**可验证的数据文件**（TOML ruleset）：
//!   - 规则是数据 → 可静态校验、可进 CI 做 golden hash、可形式化（学术主线）
//!   - 引擎 Rust 直接解释 → 更快更稳，确定性天然（无 Lua 编译中间层）
//!   - 高级/自定义规则（士气、域对抗、特殊效果）仍走 Lua 插件钩子（架构分层，不推翻）
//!
//! 确定性：ruleset 本身是规范输入。设计为：同一 ruleset → 完全相同的移动判定结果。

use std::collections::HashMap;

use serde::Deserialize;

/// 移动风格
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MoveStyle {
    /// 单步：每次推进一格（受地形代价与移动点约束）
    Step,
    /// 跳跃：直接落到 offsets 格（无视路径地形，如马/战斗跳）
    Leap,
    /// 滑行：沿方向一直走，直到阻挡/火力/移动点用尽（如车/炮直线）
    Slide,
}

/// 地形定义
#[derive(Debug, Clone, Deserialize)]
pub struct Terrain {
    pub name: String,
    /// 网格简写符号（cells 里用单字符引用，如 '.' 平原 '~' 水域）
    pub symbol: char,
    /// 进入该格所需移动点数；0 = 不可进入（水/墙）
    pub move_cost: u32,
    /// 防御加成（0-100 百分比），M2.1 火力判定用
    #[serde(default)]
    pub defense_bonus: u32,
}

/// 兵种类别（决定基础通行系数，仿 FreeCiv unit_class）
#[derive(Debug, Clone, Deserialize)]
pub struct UnitClass {
    pub name: String,
    /// 基准每格移动代价（0=不可通行）
    pub move_cost: u32,
}

/// 兵种定义
#[derive(Debug, Clone, Deserialize)]
pub struct UnitType {
    pub name: String,
    /// 所属类别（引用 unit_classes 的键）
    pub class: String,
    /// 每回合移动点数
    pub move_points: u32,
    /// 移动风格
    pub move_style: MoveStyle,
    /// 合法方向增量（leap/step 用）：(行增量, 列增量)，如马 = [[2,1],[1,2],...]
    #[serde(default)]
    pub move_offsets: Vec<(i8, i8)>,
    /// 攻击力
    #[serde(default)]
    pub attack: u32,
    /// 防御力
    #[serde(default)]
    pub defense: u32,
    /// 攻击射程（格）：1=相邻，>1=远程（炮兵/火炮）。默认 1。
    #[serde(default = "default_range")]
    pub range: u32,
}

fn default_range() -> u32 {
    1
}

/// 初始部署项
#[derive(Debug, Clone, Deserialize)]
pub struct DeployEntry {
    /// 兵种 kind（引用 units 的键）
    pub kind: String,
    /// 行、列（0-indexed）
    pub row: u8,
    pub col: u8,
    /// 归属方（0=红方, 1=蓝方, ...）
    #[serde(default)]
    pub owner: u8,
    /// 单位展示名（可选；缺省回退兵种名）
    #[serde(default)]
    pub name: Option<String>,
}

/// 可占领的要点/要地（胜利目标，动态易手）
#[derive(Debug, Clone, Deserialize)]
pub struct Objective {
    pub name: String,
    /// 所在格（行、列）
    pub row: u8,
    pub col: u8,
}

/// 胜负条件
#[derive(Debug, Clone, Deserialize)]
pub struct Victory {
    /// 需占领的要点数达到此值即胜
    #[serde(default = "default_need")]
    pub need_points: u32,
    /// 消灭指挥类单位即胜（可选：指挥官 id 列表）
    #[serde(default)]
    pub commanders: Vec<String>,
}

fn default_need() -> u32 {
    1
}

/// 地形网格（棋盘'画布'）
#[derive(Debug, Clone, Deserialize)]
pub struct TerrainGrid {
    /// 行数
    pub rows: usize,
    /// 列数
    pub cols: usize,
    /// 每格地形 id（引用 terrains 的键）。行序 0..rows, 列序 0..cols
    pub cells: Vec<String>,
}

/// 完整规则集
#[derive(Debug, Clone, Deserialize)]
pub struct Ruleset {
    pub name: String,
    /// 地形定义表：id -> Terrain
    pub terrains: HashMap<String, Terrain>,
    /// 兵种类别表：id -> UnitClass
    #[serde(default)]
    pub unit_classes: HashMap<String, UnitClass>,
    /// 兵种定义表：kind -> UnitType
    pub units: HashMap<String, UnitType>,
    /// 地形网格
    pub terrain: TerrainGrid,
    /// 初始部署
    #[serde(default)]
    pub deploy: Vec<DeployEntry>,
    /// 可占领要点（胜利目标）
    #[serde(default)]
    pub objectives: Vec<Objective>,
    /// 胜负条件
    #[serde(default)]
    pub victory: Option<Victory>,
    /// 通过 checker 校验后的结果（缓存用于渲染提示，非规则逻辑）
    #[serde(skip)]
    pub _meta: (),
}

impl Ruleset {
    /// 从 TOML 字符串解析并静态校验。校验失败返回可读错误。
    pub fn from_toml(src: &str) -> Result<Ruleset, String> {
        let rs: Ruleset = toml::from_str(src).map_err(|e| format!("ruleset 解析失败: {e}"))?;
        rs.validate()?;
        Ok(rs)
    }

    /// 静态校验：幽灵引用、非法属性、网格尺寸一致性。
    fn validate(&self) -> Result<(), String> {
        // 1. 初始部署引用的兵种必须存在
        for d in &self.deploy {
            if !self.units.contains_key(&d.kind) {
                return Err(format!(
                    "幽灵兵种: 部署引用 '{}' 但 units 未定义 (可用: {:?})",
                    d.kind,
                    self.units.keys().collect::<Vec<_>>()
                ));
            }
            // 部署坐标必须在网格内
            if d.row as usize >= self.terrain.rows || d.col as usize >= self.terrain.cols {
                return Err(format!(
                    "部署越界: '{}' @ ({},{}) 超出网格 {}x{}",
                    d.kind, d.row, d.col, self.terrain.rows, self.terrain.cols
                ));
            }
            // 部署格必须可通行（军舰类 unit 例外：可部署/逗留在水域）
            let idx = d.row as usize * self.terrain.cols + d.col as usize;
            let ch = self.terrain.cells[idx].chars().next().unwrap_or('?');
            let is_naval = self
                .units
                .get(&d.kind)
                .map(|u| u.class == "naval")
                .unwrap_or(false);
            let terrain_passable =
                self.terrains.values().any(|t| t.symbol == ch && t.move_cost > 0);
            let is_water = self.terrains.values().any(|t| t.symbol == ch && t.move_cost == 0);
            if !terrain_passable && !(is_naval && is_water) {
                return Err(format!(
                    "部署位置不可通行: '{}' @ ({},{}) 落在 '{}' 上（水域/障碍）",
                    d.kind, d.row, d.col, ch
                ));
            }
        }

        // 1b. 要点坐标必须在网格内
        for (i, obj) in self.objectives.iter().enumerate() {
            if obj.row as usize >= self.terrain.rows || obj.col as usize >= self.terrain.cols {
                return Err(format!(
                    "要点越界: 目标[{}] '{}' @ ({},{}) 超出网格 {}x{}",
                    i, obj.name, obj.row, obj.col, self.terrain.rows, self.terrain.cols
                ));
            }
        }

        // 2. 兵种引用的类别必须存在（若声明了类别体系）
        for (kind, ut) in &self.units {
            if !self.unit_classes.contains_key(&ut.class) {
                return Err(format!(
                    "幽灵类别: 兵种 '{}' 引用 class '{}' 但 unit_classes 未定义",
                    kind, ut.class
                ));
            }
        }

        // 3. 地形网格 cell 引用的符号必须存在且唯一
        let mut sym_map: HashMap<char, String> = HashMap::new();
        for (id, t) in &self.terrains {
            if sym_map.insert(t.symbol, id.clone()).is_some() {
                return Err(format!("地形符号重复: '{}' 被多个地形使用", t.symbol));
            }
        }
        for (idx, cell) in self.terrain.cells.iter().enumerate() {
            let ch = cell.chars().next().unwrap_or('?');
            if !sym_map.contains_key(&ch) {
                return Err(format!(
                    "幽灵地形: 网格第 {idx} 格引用 '{}' 但无地形使用该符号 (可用: {:?})",
                    cell,
                    sym_map.keys().collect::<Vec<_>>()
                ));
            }
        }

        // 4. 网格尺寸一致性
        if self.terrain.cells.len() != self.terrain.rows * self.terrain.cols {
            return Err(format!(
                "地形网格尺寸不匹配: cells={} 但 rows×cols={}×{}={}",
                self.terrain.cells.len(),
                self.terrain.rows,
                self.terrain.cols,
                self.terrain.rows * self.terrain.cols
            ));
        }

        // 5. 部署坐标在棋盘内
        for d in &self.deploy {
            if d.row as usize >= self.terrain.rows || d.col as usize >= self.terrain.cols {
                return Err(format!(
                    "部署越界: ({},{}) 超出 {}x{} 棋盘",
                    d.row, d.col, self.terrain.rows, self.terrain.cols
                ));
            }
        }

        Ok(())
    }

    /// 坐标 → 地形
    pub fn terrain_at(&self, row: usize, col: usize) -> &Terrain {
        let idx = row * self.terrain.cols + col;
        let ch = self.terrain.cells[idx].chars().next().unwrap_or('?');
        self.terrains
            .values()
            .find(|t| t.symbol == ch)
            .expect("validated symbol")
    }

    /// 该格是否可通行（move_cost > 0）
    pub fn passable(&self, row: usize, col: usize) -> bool {
        self.terrain_at(row, col).move_cost > 0
    }
}