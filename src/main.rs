//! M2 主程序：数据驱动规则集（Ruleset）演示
//!
//! 从一个 TOML ruleset 构建引擎：兵种/地形/移动规则/初始部署全部来自数据。
//! 验证：① ruleset 静态校验（幽灵引用/越界/尺寸） ② 数据驱动移动判定（几何+地形）
//! ③ 事件溯源确定性回放。Lua 插件保留为可选增强钩子（默认不加载）。

use std::path::{Path, PathBuf};
use std::rc::Rc;

use wargame::host::PluginRepo;
use wargame::ruleset::Ruleset;
use wargame::{Engine, Outcome};
use wargame::snapshot;

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

    // 可选渲染插件：--render <plugin_name>（从 plugins 目录加载）
    let render_plugin = args
        .windows(2)
        .find(|w| w[0] == "--render")
        .map(|w| w[1].clone());

    // —— 从规则集构建引擎 ——
    let mut eng = Engine::from_ruleset(ruleset, repo.clone());
    let cols = eng.board.cols;
    let rows = eng.board.rows;
    let rs = eng.ruleset.clone();

    println!("\n初始局面（地形 + 单位）:");
    render(&eng.board.to_units_vec(), &rs, rows, cols);
    println!("事件日志: 0 条，确定性自检: {}", eng.deterministic_check());

    // —— 根据规则集选择演示推演 ——
    let is_songhu = rs.name.contains("淞沪");
    if is_songhu {
        songhu_demo(&mut eng, &rs, rows, cols);
    } else {
        demo_demo(&mut eng, &rs, rows, cols);
    }

    // —— 可选渲染插件：终局后用稳定快照调用 ——
    if let Some(pname) = &render_plugin {
        let plugin = match repo.get(pname).or_else(|| {
            repo.load(&format!("{pname}.lua")).ok().and_then(|n| repo.get(&n))
        }) {
            Some(p) => p,
            None => {
                eprintln!("无法加载渲染插件 [{pname}]（未在 --plugins 目录找到 {pname}.lua）");
                return;
            }
        };
        // 构建稳定快照 → Lua 表 → 调 render 钩子
        let snap = snapshot::build(&eng.board, &rs, &eng.logs, eng.winner());
        let lua = plugin.lua_rc.clone();
        let st = snapshot::to_lua(&lua, &snap, &|kind: &str| kind.to_string());
        match &plugin.render {
            Some(render_fn) => {
                let v = render_fn(&st);
                match v {
                    mlua::Value::String(s) => {
                        let txt = s.to_string_lossy();
                        println!("\n── 渲染插件 [{pname}] 输出(文本) ──\n{}", txt)
                    }
                    other => {
                        use mlua::LuaSerdeExt;
                        match lua.from_value::<serde_json::Value>(other) {
                            Ok(j) => println!(
                                "\n── 渲染插件 [{pname}] 输出(JSON) ──\n{}",
                                serde_json::to_string_pretty(&j).unwrap()
                            ),
                            Err(e) => eprintln!("渲染插件返回值无法 JSON 化: {e}"),
                        }
                    }
                }
            }
            None => eprintln!("插件 [{pname}] 没有 render 钩子"),
        }
    }
}

/// 演示序列（demo.toml）：攻击/射程/移动/夺点/胜负 全链路
fn demo_demo(eng: &mut Engine, rs: &Ruleset, rows: usize, cols: usize) {
    use wargame::event::Command;
    // —— 攻击判定（M2.1 纯计算：攻防+地形，无掷骰）——
    println!("\n=== 攻击判定（数据驱动：攻防 + 射程，无掷骰） ===");

    // 贴身攻击：0方骑士#2(5,6) 攻击 1方步兵#4(4,6)
    // 骑士 attack4 vs 步兵 defense2 + 平原0 → 4>=2 → hit! 歼灭步兵#4
    let r = eng.submit(Command::Attack { unit: 2, target: 4 });
    println!(
        "骑士#2 (5,6) 攻击步兵#4 (4,6) 贴身 : {}",
        outcome_str(&r)
    );

    // 射程外攻击拒绝：1方骑士#5(1,6) 打 0方战车#3(6,6)，曼哈顿距离>range=1
    let r2 = eng.submit(Command::Attack { unit: 5, target: 3 });
    println!(
        "骑士#5 (1,6) 攻击战车#3 (6,6) 超射程 : {}",
        outcome_str(&r2)
    );

    // —— 数据驱动移动测试 ——
    println!("\n=== 移动判定（数据驱动：几何 + 地形） ===");

    // 骑士 leap 跳入水域非法（落点须可通行）——骑士#2 仍在(5,6)
    let r_leap_water = eng.submit(Command::Move { unit: 2, to: eng.board.cell_at(4, 4) });
    println!(
        "骑士#2 (5,6)→(4,4) 马跳落水域 : {}",
        outcome_str(&r_leap_water)
    );

    // 骑士 leap 合法：跨水域跳到陆地(3,5)
    let r3 = eng.submit(Command::Move { unit: 2, to: eng.board.cell_at(3, 5) });
    println!(
        "骑士#2 (5,6)→(3,5) 马跳跨水落平原 : {}",
        outcome_str(&r3)
    );

    // 战车 slide 直线（向下无阻挡）
    let r4 = eng.submit(Command::Move { unit: 3, to: eng.board.cell_at(7, 6) });
    println!(
        "战车#3 (6,6)→(7,6) 直线滑行 : {}",
        outcome_str(&r4)
    );

    println!("\n=== 非法走法（数据驱动拒绝） ===");
    // 骑士非马跳
    let r5 = eng.submit(Command::Move { unit: 5, to: eng.board.cell_at(2, 6) });
    println!(
        "骑士#5 (1,6)→(2,6) 竖一格非马跳 : {}",
        outcome_str(&r5)
    );
    // 战车斜线（slide 非直线）
    let r6 = eng.submit(Command::Move { unit: 6, to: eng.board.cell_at(1, 6) });
    println!(
        "战车#6 (0,5)→(1,6) 斜线非法 : {}",
        outcome_str(&r6)
    );

    // —— 夺点 + 胜负（M2.1）——
    println!("\n=== 夺点 → 胜负判定（need_points 达标即胜） ===");

    // 0方步兵#1(6,3) 两连步走向中央高地(5,4)，占领即胜
    let r7 = eng.submit(Command::Move { unit: 1, to: eng.board.cell_at(5, 3) });
    println!(
        "步兵#1 (6,3)→(5,3) 接近高地 : {}",
        outcome_str(&r7)
    );
    let r8 = eng.submit(Command::Move { unit: 1, to: eng.board.cell_at(5, 4) });
    println!(
        "步兵#1 (5,3)→(5,4) 占领中央高地 : {}",
        outcome_str(&r8)
    );

    finish(&eng, rs, rows, cols);
}

/// 演示序列（songhu.toml）：淞沪会战——多点争夺 → 胜负判定
fn songhu_demo(eng: &mut Engine, rs: &Ruleset, rows: usize, cols: usize) {
    use wargame::event::Command;
    println!("\n=== 淞沪会战推演（多点争夺战：国军 4 点达标，日军固守吴淞） ===");

    // 初始占领盘点：第3师团(1,17)钉在吴淞码头 → 日军已占 1 点
    println!("〔日军〕第3师团固守吴淞码头（已在要点格），虹口/罗店/大场/闸北皆兵临城下未定。");
    println!("当前占领: {:?}", eng.board.objective_owners(rs));

    // 日军沿江推进：战车联队#8 (2,17)→(3,17) 抢滩向市区压
    let t = eng.submit(Command::Move { unit: 8, to: eng.board.cell_at(3, 17) });
    println!("〔日军〕战车第1中队 (2,17)→(3,17) 沿江推进 : {}", outcome_str(&t));

    // 国军多点反击夺占——
    // 88师#1 北上： (12,17)→(11,17)→(11,16) 夺苏州河渡口
    let a = eng.submit(Command::Move { unit: 1, to: eng.board.cell_at(11, 17) });
    println!("〔国军〕88师 (12,17)→(11,17) 挺进渡口 : {}", outcome_str(&a));
    let a2 = eng.submit(Command::Move { unit: 1, to: eng.board.cell_at(11, 16) });
    println!("〔国军〕88师 夺占苏州河渡口 : {}", outcome_str(&a2));

    // 87师#2 (9,9)→(9,8) 夺大场
    let b = eng.submit(Command::Move { unit: 2, to: eng.board.cell_at(9, 8) });
    println!("〔国军〕87师 (9,9)→(9,8) 夺大场阵地 : {}", outcome_str(&b));

    // 炮兵#3 (10,10)→(10,9) 夺闸北
    let c = eng.submit(Command::Move { unit: 3, to: eng.board.cell_at(10, 9) });
    println!("〔国军〕炮兵第2旅 (10,10)→(10,9) 占闸北 : {}", outcome_str(&c));

    // 地方军#4 (7,4)→(7,5) 夺罗店
    let d = eng.submit(Command::Move { unit: 4, to: eng.board.cell_at(7, 5) });
    println!("〔国军〕地方守备旅 (7,4)→(7,5) 夺罗店 : {}", outcome_str(&d));

    finish(eng, rs, rows, cols);
}

/// 共用的终局输出：渲染 + 胜负 + 确定性 + 哈希
fn finish(eng: &Engine, rs: &Ruleset, rows: usize, cols: usize) {
    println!("\n--- 终局 ---");
    render(&eng.board.to_units_vec(), rs, rows, cols);
    if let Some(winner) = eng.winner() {
        let who = if winner == 0 { "国民政府军(owner0)" } else { "日本军队(owner1)" };
        let role = if winner == 0 { "抗敌士气高涨" } else { "攻势凌厉" };
        println!("🏆 胜者: {}（{}）", who, role);
    } else {
        println!("（未分胜负——尚无一方达成 victory.need_points）");
    }
    println!("已占领要点: {:?}", eng.board.objective_owners(rs));
    println!("事件日志: {} 条", eng.logs.len());
    println!("确定性自检 (replay==board): {}", eng.deterministic_check());
    println!("日志 SHA-256: {}", eng.logs_hash());
    println!("\nM2 验证完成：规则集静态校验 + 数据驱动移动/攻击/夺点判定（几何/地形/攻防） + 确定性回放 均通过 ✅");
}

fn outcome_str(o: &Outcome) -> String {
    match o {
        Outcome::Applied { event } => format!("✅ 通过，事件: {event:?}"),
        Outcome::Rejected { reason } => format!("❌ 拒绝: {reason}"),
    }
}

/// 渲染：地形网格 + 单位（多兵种字符映射）
fn render(units: &[(u16, &wargame::event::Unit)], rs: &Ruleset, rows: usize, cols: usize) {
    // 地形符号：直接用规则集里每个 terain 的 symbol 字段（数据驱动）
    let grid: Vec<Vec<char>> = (0..rows)
        .map(|r| {
            (0..cols)
                .map(|c| {
                    let t = rs.terrain_at(r, c);
                    t.symbol
                })
                .collect()
        })
        .collect();

    // 单位覆盖在地形上（未知兵种退首位字符）
    let mut unit_chars = vec![vec![' '; cols]; rows];
    for (cell, u) in units {
        let (r, c) = ((*cell as usize) / cols, (*cell as usize) % cols);
        let ch = match u.kind.as_str() {
            "knight" => '♞',
            "rook" => '♜',
            "infantry" => '♟',
            "cn_inf" => '人',
            "cn_arty" => '砲',
            "cn_local" => '勇',
            "jp_div" => '兵',
            "jp_tank" => '坦',
            "jp_navy" => '舰',
            _ => u.kind.chars().next().unwrap_or('?'),
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