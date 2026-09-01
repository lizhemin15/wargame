//! 确定性引擎核心：把 命令 →(裁决管线)→ 事件 → 折叠状态
//!
//! M2 裁决管线（M1 Lua 规则即插件 → 数据驱动 ruleset 优先）：
//!   submit(Move)
//!     → 定位单位
//!     → ① 数据驱动移动判定（ruleset.can_move：兵种几何 + 地形 + 移动点）—— 主闸
//!     → ② Lua 自定义插件钩子（可选：士气/域效果/特殊规则）—— 增强层
//!     → 全过 → MoveAccepted 事件；若落点为要点 → 追加 PointTaken；判定胜负
//!   submit(Attack)
//!     → 定位攻/守单位
//!     → ① 数据驱动攻击判定（射程 in_range + 攻防攻击力比较）—— 主闸
//!     → 命中间接 → AttackResolved(hit) + Eliminated；检查吞指挥官胜负
//!
//! 确定性保证：
//!   - logs 是唯一事实源；状态由 replay(logs) 折叠，绝不直接改
//!   - ruleset 是规范数据输入；判定纯函数无 IO/随机
//!   - 引擎不依赖任何非确定性输入

use sha2::{Digest, Sha256};
use std::rc::Rc;

use crate::board::Board;
use crate::combat;
use crate::event::{Cell, Command, Event, Unit};
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
    /// 是否已分出胜负（GameOver 后拒绝所有命令）
    pub over: bool,
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
            over: false,
        }
    }

    /// 追加事件并折叠（唯一入口，保证日志与状态同步）。
    fn push(&mut self, ev: Event) {
        self.logs.push(ev.clone());
        self.board.apply(&ev);
    }

    /// 提交一条命令。
    pub fn submit(&mut self, cmd: Command) -> Outcome {
        if self.over {
            return Outcome::Rejected {
                reason: "game over".to_string(),
            };
        }
        match cmd {
            Command::Move { unit, to } => self.submit_move(unit, to),
            Command::Attack { unit, target } => self.submit_attack(unit, target),
        }
    }

    fn submit_move(&mut self, unit_id: crate::event::UnitId, to: Cell) -> Outcome {
        let Some(unit) = self.board.get(unit_id).cloned() else {
            return Outcome::Rejected {
                reason: format!("unknown unit: {}", unit_id),
            };
        };
        let from = unit.cell;

        // —— ① 数据驱动移动判定（主闸）——
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

        // 全过 → 固化移动事件
        let event = Event::MoveAccepted { unit: unit.id, from, to };
        self.push(event.clone());

        // 若落点是要点且无守军 → 占领要点
        if self.ruleset
            .objectives
            .iter()
            .enumerate()
            .any(|(i, o)| {
                self.board.cell_at(o.row as usize, o.col as usize) == to
                    && self.board.point_owner.get(&i).map_or(true, |&o| o != unit.owner)
            })
        {
            if let Some((i, _)) = self
                .ruleset
                .objectives
                .iter()
                .enumerate()
                .find(|(_, o)| self.board.cell_at(o.row as usize, o.col as usize) == to)
            {
                self.push(Event::PointTaken { point: i, owner: unit.owner });
            }
        }

        self.check_win();
        Outcome::Applied { event }
    }

    fn submit_attack(&mut self, unit_id: crate::event::UnitId, target_id: crate::event::UnitId) -> Outcome {
        let Some(attacker) = self.board.get(unit_id).cloned() else {
            return Outcome::Rejected {
                reason: format!("unknown unit: {}", unit_id),
            };
        };
        let Some(defender) = self.board.get(target_id).cloned() else {
            return Outcome::Rejected {
                reason: format!("unknown unit: {}", target_id),
            };
        };
        if attacker.owner == defender.owner {
            return Outcome::Rejected {
                reason: "cannot attack own unit".to_string(),
            };
        }
        // 攻击者自身 cell 判射程
        if !combat::in_range(&self.ruleset, &attacker.kind, attacker.cell, defender.cell, &self.board) {
            return Outcome::Rejected {
                reason: format!(
                    "out of range: {} at {:?} vs {} at {:?}",
                    attacker.kind,
                    self.board.cell_pos(attacker.cell),
                    defender.kind,
                    self.board.cell_pos(defender.cell)
                ),
            };
        }

        let hit = combat::attack_hits(&self.ruleset, &attacker.kind, &defender, &self.board);
        let ev = Event::AttackResolved {
            attacker: attacker.id,
            defender: defender.id,
            defender_cell: defender.cell,
            hit,
        };
        self.push(ev.clone());

        if hit {
            self.push(Event::Eliminated {
                unit: defender.id,
                cell: defender.cell,
            });
        }

        self.check_win();
        Outcome::Applied { event: ev }
    }

    /// 每步后检查胜负；命中则 push GameOver 并置 over=true。
    fn check_win(&mut self) {
        if self.over {
            return;
        }
        if let Some(winner) = combat::check_victory(&self.ruleset, &self.board) {
            let reason = format!("player {} reaches victory objective(s)", winner);
            self.push(Event::GameOver {
                winner,
                reason: reason.clone(),
            });
            self.over = true;
        }
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

    /// 胜者（若有）
    pub fn winner(&self) -> Option<u8> {
        self.logs
            .iter()
            .find_map(|ev| match ev {
                Event::GameOver { winner, .. } => Some(*winner),
                _ => None,
            })
    }

    #[allow(dead_code)]
    fn _hold_unit(_u: &Unit) {}
}