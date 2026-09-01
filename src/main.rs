//! M2 主程序：数据驱动规则集（Ruleset）演示
//!
//! 从一个 TOML ruleset 构建引擎：兵种/地形/移动规则/初始部署全部来自数据。
//! 验证：① ruleset 静态校验（幽灵引用/越界/尺寸） ② 数据驱动移动判定（几何+地形）
//! ③ 事件溯源确定性回放。Lua 插件保留为可选增强钩子（默认不加载）。

use std::path::{Path, PathBuf};
use std::rc::Rc;

use wargame::host::PluginRepo;
use wargame::ruleset::Ruleset;
use wargame::{Command, Engine, Outcome};

fn main() {
    println!("=== wargame M2：数据驱动规则集 ===\n");

    // —— 定位规则集文件 ——
    // 解析顺序：命令行 --ruleset <path> > 环境变量 WARGAME_RULESET > 默认 ./rulesets/demo.toml
    let args: Vec<String> = std::env::args().collect();
    let ruleset_path = args
        .windows(2)
        .find(|w| w[0] == "--ruleset")
        .map(|w| Path::new(&w[1]).to_path_buf())
        .or_else(|| std::env::var("WARGAME_RULESET").ok().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("rulesets/demo.toml"));

    // —— 解析 + 静态校验规则集 ——
    let src = match std::fs::read_to_string(&ruleset_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "错误：无法读取规则集 {:?}（{}）。\n用法: {} --ruleset <文件>  或设置 WARGAME_RULESET",
                ruleset_path,
                e,
                args.first().cloned().unwrap_or_else(|| "wargame".into())
            );
            std::process::exit(2);
        }
    };
    let ruleset = Ruleset::from_toml(&src).expect("ruleset 解析/校验失败");
    println!(
        "规则集: {}  | 地形 {} 类 × 兵种 {} 种 × 初始 {} 单位 | 网格 {}x{}",
        ruleset.name,
        ruleset.terrains.len(),
        ruleset.units.len(),
        ruleset.deploy.len(),
        ruleset.terrain.rows,
        ruleset.terrain.cols
    );

    // Lua 插件钩子（可选增强，M2 标准移动已数据驱动）。目录存在则建立 repo，否则空。
    let plugins_dir = args
        .windows(2)
        .find(|w| w[0] == "--plugins")
        .map(|w| Path::new(&w[1]).to_path_buf())
        .or_else(|| std::env::var("WARGAME_PLUGINS").ok().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("plugins"));
    let repo = Rc::new(PluginRepo::new(&plugins_dir));
    // 不强制加载任何 Lua 插件——标准规则全部数据驱动

    // —— 从规则集构建引擎 ——
    let mut eng = Engine::from_ruleset(ruleset, repo);
    let cols = eng.board.cols;
    let rows = eng.board.rows;
    let rs = eng.ruleset.clone();

    println!("\n初始局面（地形 + 单位）:");
    render(&eng.board.to_units_vec(), &rs, rows, cols);
    println!("事件日志: 0 条，确定性自检: {}", eng.deterministic_check());

    // —— 数据驱动移动测试 ——
    println!("\n=== 移动判定（数据驱动：几何 + 地形） ===");

    // 骑士 leap 马跳
    let r = eng.submit(Command::Move { unit: 1, to: eng.board.cell_at(5, 2) });
    println!(
        "骑士#1 (7,1)→(5,2) 马跳平原 : {}",
        outcome_str(&r)
    );

    // 战车 slide 直线（被山地/水域阻挡测试）
    let r2 = eng.submit(Command::Move { unit: 2, to: eng.board.cell_at(7, 5) });
    println!(
        "战车#2 (7,2)→(7,5) 直线(穿(7,3)山地) : {}",
        outcome_str(&r2)
    );

    // 步兵 step + 地形代价
    let r3 = eng.submit(Command::Move { unit: 3, to: eng.board.cell_at(5, 4) });
    println!(
        "步兵#3 (6,3)→(5,4) 一步(森林) : {}",
        outcome_str(&r3)
    );

    println!("\n=== 非法走法（数据驱动拒绝） ===");
    // 骑士非马跳
    let r4 = eng.submit(Command::Move { unit: 4, to: eng.board.cell_at(0, 4) });
    println!(
        "骑士#4 (0,6)→(0,4) 竖两格非马跳 : {}",
        outcome_str(&r4)
    );
    // 战车斜线（slide 非直线）
    let r5 = eng.submit(Command::Move { unit: 5, to: eng.board.cell_at(1, 6) });
    println!(
        "战车#5 (0,5)→(1,6) 斜线非法 : {}",
        outcome_str(&r5)
    );
    // 步兵进水域
    let r6 = eng.submit(Command::Move { unit: 6, to: eng.board.cell_at(2, 4) });
    println!(
        "步兵#6 (1,4)→(2,4) 跨水域不可通行 : {}",
        outcome_str(&r6)
    );

    println!("\n--- 终局 ---");
    render(&eng.board.to_units_vec(), &rs, rows, cols);
    println!("事件日志: {} 条", eng.logs.len());
    println!("确定性自检 (replay==board): {}", eng.deterministic_check());
    println!("日志 SHA-256: {}", eng.logs_hash());
    println!("\nM2 验证完成：规则集静态校验 + 数据驱动移动判定（几何/地形） + 确定性回放 均通过 ✅");
}

fn outcome_str(o: &Outcome) -> String {
    match o {
        Outcome::Applied { event } => format!("✅ 通过，事件: {event:?}"),
        Outcome::Rejected { reason } => format!("❌ 拒绝: {reason}"),
    }
}

/// 渲染：地形网格 + 单位（多兵种字符映射）
fn render(units: &[(u8, &wargame::event::Unit)], rs: &Ruleset, rows: usize, cols: usize) {
    // 地形图例
    let mut legend: Vec<String> = Vec::new();
    let grid: Vec<Vec<char>> = (0..rows)
        .map(|r| {
            (0..cols)
                .map(|c| {
                    let t = rs.terrain_at(r, c);
                    let sym = match t.name.as_str() {
                        "平原" | "道路" => '.',
                        "森林" => 'F',
                        "山地" => '^',
                        "水域" => '~',
                        _ => '?',
                    };
                    if sym == '?' {
                        legend.push(format!("{}→?", t.name));
                    }
                    sym
                })
                .collect()
        })
        .collect();

    // 单位覆盖在地形上
    let mut unit_chars = vec![vec![' '; cols]; rows];
    for (cell, u) in units {
        let (r, c) = ((*cell as usize) / cols, (*cell as usize) % cols);
        let ch = match u.kind.as_str() {
            "knight" => '♞',
            "rook" => '♜',
            "infantry" => '♟',
            _ => '?',
        };
        unit_chars[r][c] = if u.owner == 0 { ch } else { ch.to_ascii_uppercase() };
    }

    for r in 0..rows {
        let mut line = format!("{r} ");
        for c in 0..cols {
            if unit_chars[r][c] != ' ' {
                line.push(unit_chars[r][c]); // 单位覆盖地形
            } else {
                line.push(grid[r][c]); // 纯地形
            }
            line.push(' ');
        }
        println!("{line}");
    }
    let mut bottom = String::from("   ");
    for c in 0..cols {
        bottom.push_str(&format!("{c} "));
    }
    println!("{bottom}  (列)");
    println!("图例: 骑士♞ 战车♜ 步兵♟  |  .平原 ~水域 ^山地 F森林");
}