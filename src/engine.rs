//! 确定性引擎核心：把 命令 →(裁决管线)→ 事件 → 折叠状态
//!
//! M2 裁决管线（M1 Lua 规则即插件 → 数据驱动 ruleset 优先）：
//!   submit(Command)
//!     → 定位单位
//!     → ① 数据驱动判定（ruleset.can_move：兵种几何 + 地形 + 移动点）—— 主闸
//!     → ② Lua 自定义插件钩子（可选：士气/域效果/特殊规则）—— 增强层
//!     → 全过 → 生成 MoveAccepted 事件，append 到日志，折叠进状态
//!     → 任一不过 → 拒绝，无事件，状态不变
//!
//! 确定性保证：
//!   - logs 是唯一事实源；状态由 replay(logs) 折叠，绝不直接改
//!   - ruleset 是规范数据输入；判定纯函数无 IO/随机
//!   - 引擎不依赖任何非确定性输入

use sha2::{Digest, Sha256};
use std::rc::Rc;

use crate::board::Board;
use crate::event::{Command, Event};
use crate::host::PluginRepo;
use crate::move_rules;
use crate::ruleset::Ruleset;

/// 一次提交的结果
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// 命令合法，产生事件并已折叠
    Applied { event: Event },
    /// 命令非法（裁决管线拒绝或单位不存在），无事件产生
    Rejected { reason: String },
}

/// 确定性引擎：持有一份事件日志 + 可折叠状态。
pub struct Engine {
    pub logs: Vec<Event>,
    pub board: Board,
    pub ruleset: Ruleset,
    pub plugins: Rc<PluginRepo>,
    /// Lua 自定义插件钩子（增强层，可空）。数据驱动判定后再叠这些。
    pub endorse_order: Vec<String>,
}

impl Engine {
    /// 从 ruleset 构建引擎（初始棋盘来自 ruleset.deploy 数据）。
    pub fn from_ruleset(ruleset: Ruleset, plugins: Rc<PluginRepo>) -> Self {
        let board = Board::from_ruleset(&ruleset);
        Self {
            logs: Vec::new(),
            board,
            ruleset,
            plugins,
            // Lua 插件钩子默认空（M2 标准移动已数据驱动），按需挂载
            endorse_order: Vec::new(),
        }
    }

    /// 提交一条命令（裁决管线）。
    pub fn submit(&mut self, cmd: Command) -> Outcome {
        let (unit_id, to) = match &cmd {
            Command::Move { unit, to } => (*unit, *to),
        };

        let Some(unit) = self.board.get(unit_id).cloned() else {
            return Outcome::Rejected { reason: format!("unknown unit: {}", unit_id) };
        };
        let from = unit.cell;

        // —— ① 数据驱动判定（主闸）——
        match move_rules::can_move_on_board(&self.ruleset, &self.board, unit_id, to) {
            crate::move_rules::MoveVerdict::Rejected(reason) => {
                return Outcome::Rejected { reason };
            }
            crate::move_rules::MoveVerdict::Ok => {}
        }

        // —— ② Lua 自定义插件钩子（可选增强）——
        let mut reasons = Vec::new();
        for pname in &self.endorse_order {
            let plugin_rc = match self.plugins.get(pname) {
                Some(p) => p,
                None => {
                    return Outcome::Rejected {
                        reason: format!("plugin '{}' not loaded", pname),
                    }
                }
            };
            let ok = (plugin_rc.can_move)(&unit, from, to, &self.board);
            if !ok {
                reasons.push(pname.clone());
            }
        }

        if !reasons.is_empty() {
            return Outcome::Rejected {
                reason: format!("rejected by plugin(s): {:?}", reasons),
            };
        }

        // 全过 → 固化事件
        let event = Event::MoveAccepted { unit: unit.id, from, to };
        self.logs.push(event.clone());
        self.board.apply(&event);
        Outcome::Applied { event }
    }

    /// 从事件日志重放重建状态（确定性校验）。
    pub fn replay(&self) -> Board {
        let mut b = Board::from_ruleset(&self.ruleset);
        for ev in &self.logs {
            b.apply(ev);
        }
        b
    }

    /// 确定性自检：当前 board == 日志重放 board？
    pub fn deterministic_check(&self) -> bool {
        self.board.equivalent(&self.replay())
    }

    /// 事件日志规范流的 SHA-256（golden 值，可进 CI 断言）
    pub fn logs_hash(&self) -> String {
        let mut h = Sha256::new();
        for ev in &self.logs {
            // 确定性：serde 序列化 enum/struct，无随机/墙钟
            h.update(serde_json::to_string(ev).expect("event serializable"));
        }
        format!("{:x}", h.finalize())
    }
}