//! Web 游戏服务器（W3：前端展示 = 渲染插件）
//!
//! 与 terminal_render / to_json 并列的第三种"渲染插件"：前端页面消费**稳定快照**。
//! - GET  /              → 前端页面（内嵌 HTML，单二进制部署，离线友好）
//! - GET  /api/meta      → ruleset 元数据（规则名/尺寸/兵种图例/胜负条件/占领要点）
//! - GET  /api/state     → 当前稳定快照 JSON（snapshot::build → to_json）
//! - POST /api/move      → 提交命令 {unit,to} 或 {unit,target}，返回 {outcome,state}
//! - POST /api/reset     → 重建引擎，回到初始局面
//! - GET  /api/logs      → 事件日志（中文可读化）
//!
//! 线程模型注记：Engine 内含 Rc<PluginRepo>（非 Send）。故这里用 tiny_http 的
//! **单线程** accept 循环（一次处理一个请求），完全避开 Send/Sync 约束。
//! 单人推演游戏，单线程吞吐足够。

use std::rc::Rc;

use tiny_http::{Header, Method, Request, Response, Server};

use crate::engine::Engine;
use crate::event::Command;
use crate::ruleset::Ruleset;
use crate::snapshot;
use crate::host::PluginRepo;
/// 内嵌前端页面（单文件部署；ruleset 仍为外部数据，功能数据分离）
pub const INDEX_HTML: &str = include_str!("../web/index.html");

/// 启动 web 服务器并阻塞处理请求（单线程 accept 循环）。
/// `ruleset_src` 为 TOML 原文，reset 时用它重建引擎。
pub fn run(addr: String, ruleset_src: &str, plugins_dir: String) -> Result<(), String> {
    let server = Server::http(&addr).map_err(|e| format!("监听 {addr} 失败: {e}"))?;
    eprintln!("[wargame serve] 已监听 {} (ruleset 已解析)", addr);

    // —— 初始引擎（持有在循环外，单线程内可变）——
    let mut eng = build_engine(ruleset_src, &plugins_dir)?;

    loop {
        let Ok(mut req) = server.recv() else {
            continue;
        };
        let url = req.url().to_string();
        let method = req.method().clone();

        let resp = handle(&mut eng, ruleset_src, &plugins_dir, method, &url, &mut req);

        // 统一打 CORS 头（方便将来独立前端跨域调试）
        let resp = resp.with_header(Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap());
        let _ = req.respond(resp);
    }
}

/// 从 ruleset TOML 构建引擎
fn build_engine(ruleset_src: &str, plugins_dir: &str) -> Result<Engine, String> {
    let ruleset = Ruleset::from_toml(ruleset_src).map_err(|e| e.to_string())?;
    let repo = Rc::new(PluginRepo::new(plugins_dir));
    Ok(Engine::from_ruleset(ruleset, repo))
}

/// 快照 JSON（当前引擎状态）
fn state_json(eng: &Engine) -> String {
    snapshot::build(&eng.board, &eng.ruleset, &eng.logs, eng.winner()).to_json()
}

fn handle(
    eng: &mut Engine,
    ruleset_src: &str,
    plugins_dir: &str,
    method: Method,
    url: &str,
    req: &mut Request,
) -> Response<std::io::Cursor<Vec<u8>>> {
    // 解析路径（去掉 query string）
    let path = url.split('?').next().unwrap_or(url).to_string();

    match (method, path.as_str()) {
        (Method::Get, "/") => text(INDEX_HTML, "text/html; charset=utf-8"),
        (Method::Get, "/index.html") => text(INDEX_HTML, "text/html; charset=utf-8"),

        (Method::Get, "/api/state") => json_str(&state_json(eng)),

        (Method::Get, "/api/meta") => json_str(&meta_json(eng)),

        (Method::Get, "/api/logs") => json_str(&logs_json(eng)),

        (Method::Post, "/api/reset") => {
            *eng = match build_engine(ruleset_src, plugins_dir) {
                Ok(e) => e,
                Err(msg) => return json_obj(&format!("{{\"ok\":false,\"error\":{}}}", json_esc(&msg))),
            };
            json_obj(&format!("{{\"ok\":true,\"state\":{}}}", &state_json(eng)))
        }

        (Method::Post, "/api/move") => {
            // 读 body
            let mut body = String::new();
            let _ = std::io::Read::read_to_string(req.as_reader(), &mut body);
            match handle_move(eng, &body) {
                Ok(out) => out,
                Err(msg) => json_obj(&format!("{{\"ok\":false,\"error\":{}}}", json_esc(&msg))),
            }
        }

        (Method::Options, _) => {
            // CORS 预检
            Response::from_string("").with_status_code(204)
        }

        _ => text("404 not found: use GET /, /api/state, /api/meta, /api/logs, POST /api/move, /api/reset", "text/plain; charset=utf-8"),
    }
}

/// 处理 /api/move。body 形如 {"kind":"move","unit":3,"to":77} 或 {"kind":"attack","unit":3,"target":5}
fn handle_move(eng: &mut Engine, body: &str) -> Result<Response<std::io::Cursor<Vec<u8>>>, String> {
    #[derive(serde::Deserialize)]
    struct MoveIn {
        #[serde(default = "default_kind")]
        kind: String,
        unit: u8,
        to: Option<u16>,
        target: Option<u8>,
    }
    fn default_kind() -> String {
        "move".into()
    }

    let mi: MoveIn = serde_json::from_str(body).map_err(|e| format!("bad body: {e}"))?;
    let cmd = if mi.kind == "attack" {
        let target = mi.target.ok_or("attack 需 target")?;
        Command::Attack { unit: mi.unit, target }
    } else {
        let to = mi.to.ok_or("move 需 to")?;
        Command::Move { unit: mi.unit, to }
    };

    let outcome = eng.submit(cmd.clone());
    let (ok, message) = match &outcome {
        crate::Outcome::Applied { .. } => (true, "applied".to_string()),
        crate::Outcome::Rejected { reason } => (false, reason.clone()),
    };

    let body = format!(
        "{{\"ok\":{},\"message\":{},\"state\":{}}}",
        ok,
        json_esc(&message),
        state_json(eng),
    );
    Ok(json_obj(&body))
}

/// /api/meta：前端渲染所需的规则集元数据
fn meta_json(eng: &Engine) -> String {
    let rs = &eng.ruleset;
    // 兵种图例：kind -> {name, move_points}，供前端显示兵种能力
    let mut kinds = serde_json::Map::new();
    for (k, t) in &rs.units {
        kinds.insert(
            k.clone(),
            serde_json::json!({
                "name": t.name,
            }),
        );
    }
    // 带坐标的初始部署（前端高亮/提示用）
    let deploy: Vec<serde_json::Value> = rs
        .deploy
        .iter()
        .map(|d| {
            serde_json::json!({
                "kind": d.kind,
                "row": d.row,
                "col": d.col,
                "owner": d.owner,
                "name": d.name.clone().unwrap_or_default(),
            })
        })
        .collect();

    let unit_classes: Vec<serde_json::Value> = rs
        .unit_classes
        .iter()
        .map(|(k, uc)| {
            serde_json::json!({
                "class": k,
                "name": uc.name,
                "move_cost": uc.move_cost,
            })
        })
        .collect();

    serde_json::to_string(&serde_json::json!({
        "name": rs.name,
        "rows": rs.terrain.rows,
        "cols": rs.terrain.cols,
        "need_points": rs.victory.as_ref().map(|v| v.need_points).unwrap_or(1),
        "unit_classes": unit_classes,
        "units": kinds,
        "deploy": deploy,
    }))
    .unwrap_or_default()
}

/// /api/logs：事件日志（中文可读化）
fn logs_json(eng: &Engine) -> String {
    let rows: Vec<String> = eng
        .logs
        .iter()
        .enumerate()
        .map(|(i, e)| format!("[{i}] {e:?}"))
        .collect();
    serde_json::to_string(&serde_json::json!({
        "count": rows.len(),
        "logs": rows,
        "winner": eng.winner(),
    }))
    .unwrap_or_default()
}

// ---------- 响应构造小工具 ----------

fn json_str(s: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    text(s, "application/json; charset=utf-8")
}

fn json_obj(s: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    text(s, "application/json; charset=utf-8")
}

fn text(s: &str, ctype: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_string(s.to_string()).with_header(
        Header::from_bytes(&b"Content-Type"[..], ctype.as_bytes()).unwrap(),
    )
}

fn json_esc(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}