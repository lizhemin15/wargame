//! 确定性引擎核心：把 命令 →(Lua 裁决管线)→ 事件 → 折叠状态
//!
//! 裁决管线（纯同步，无 IO/随机/墙钟）：
//!   submit(Command)
//!     → 定位单位，取其兵种插件
//!     → 依次执行裁决管线 [兵种插件, 裁判插件, ...]（注册顺序）
//!     → 全过 → 生成 MoveAccepted 事件，append 到日志，折叠进状态
//!     → 任一不过 → 拒绝，无事件，状态不变
//!
//! 规则即插件：改/加规则 = 增删管道里的插件，Rust 内核零改动。
//!
//! 确定性保证：
//!   - logs 是唯一事实源；状态由 replay(logs) 折叠，绝不直接改
//!   - 引擎不依赖任何非确定性输入

use sha2::{Digest, Sha256};
use std::rc::Rc;

use crate::board::Board;
use crate::event::{Command, Event};
use crate::host::PluginRepo;

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
    pub plugins: Rc<PluginRepo>,
    /// 裁决管线：单位兵种插件 + 裁判插件（未来可扩展顺序）
    pub endorse_order: Vec<String>,
}

impl Engine {
    pub fn new(board: Board, plugins: Rc<PluginRepo>) -> Self {
        Self {
            logs: Vec::new(),
            board,
            plugins,
            // 默认管线：裁判先，兵种后（顺序决定语义）
            endorse_order: vec!["judge".into(), "knight".into()],
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

        // —— 裁决管线 ——
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
        let mut b = Board::initial();
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
            let line = match ev {
                Event::MoveAccepted { unit, from, to } => {
                    format!("move:{}:{}:{}", unit, from, to)
                }
            };
            h.update(line.as_bytes());
            h.update(b"\n");
        }
        format!("{:x}", h.finalize())
    }

    /// 当前状态规范 JSON 的 SHA-256
    pub fn board_hash(&self) -> String {
        let mut h = Sha256::new();
        h.update(self.board.to_canonical_json().as_bytes());
        format!("{:x}", h.finalize())
    }
}