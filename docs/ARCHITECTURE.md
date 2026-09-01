# Wargame — 架构设计（v0.1 · M1）

> 兵棋系统。Rust 内核 + Lua「规则即插件」+ 事件溯源。
> 目标：轻量、跨平台、稳定、多方以插件参与研发。

---

## 0. 技术定位

| 项 | 决策 | 理由 |
|---|---|---|
| 语言 | **Rust**（内核） | 安全、单二进制、跨平台、性能强 |
| 脚本 | **Lua 5.4**（规则/插件） | 轻量、嵌入友好、热插拔、兵棋规则天然脚本化 |
| 绑定 | **mlua**（vendored） | 免系统依赖，单二进制 |
| 状态 | **事件溯源**（Event Sourcing） | 回放、审计、确定性测试、AI 对齐 |
| 工程 | 单 crate + 模块化 | 「如无必要勿增实体」，M1 不拆 workspace |

---

## 1. 核心思想：规则即插件

**兵棋的本质是规则的堆叠**（移动、攻击、地形、士气、天气……）。把每条规则做成一个**独立 Lua 插件**，内核只负责「调度 + 固化状态」，规则全部外置可插拔。

```
        ┌─────────────────────────────────────────────┐
        │                   内核 (Rust)                │
        │                                             │
        │  Command ─▶ Dictate Pipeline ─▶ Event       │
        │  ── submit()   ｜ plugin.A │  ── apply()    │
        │                ｜ plugin.B │                │
        │                ｜ plugin.C │                │
        └────────────────┼───────────┴────────────────┘
                         │ Rc<RefCell<PluginRepo>> 共享
              ┌──────────▼───────────┐
              │   PluginRepo (Rust)   │
              │  name → Rc<Plugin>    │
              └──────────┬───────────┘
                         │ hot_reload：原子替换 Rc
              ┌──────────▼───────────┐
              │  plugins/*.lua (磁盘) │
              │  knight / judge / …  │
              └──────────────────────┘
```

**热插拔语义**：改 Lua 文件 → `hot_reload(name)` 重读磁盘 → 构造新 `Rc<Plugin>` → `RefCell` 原子替换 → **运行中的引擎无感知地拿到新规则**。

---

## 2. 分层架构

```
层                     职责                                       状态
─────────────────────────────────────────────────────────────────────
interface             CLI / (未来)HTTP / AI 接口                  无
engine                裁决管线：命令 → 插件链 → 事件              持有状态
board                 棋盘/单位世界状态（BTreeMap, 纯函数折叠）   无状态*
event                 命令 & 事件的类型定义（序列化友好）          无状态
host                  Lua 插件宿主：加载 / 热插拔 / 契约           无状态
plugins/*.lua         规则实现（兵种、裁判、地形、战机…）          外置数据
state.log             事件日志（append-only）                     持久化
```

\* `board` 本身是持有状态的数据结构，但 `apply()` 折叠是纯函数 → 可任意回放。

---

## 3. 事件溯源契约（确定性铁律）

系统只接受「命令」，命令经裁决后变成「事件」追加进日志，世界状态 = 事件序列的纯折叠。

**五个铁律**（从源头保证确定性）：

1. **命令/事件不可变**：一旦入日志，永不改写（append-only）。
2. **纯函数折叠**：`Board::apply(&event)` 无副作用，任意次调用结果一致。
3. **确定性裁决**：裁决只依赖「当前状态 + 命令 + 插件纯函数」，不依赖时间/随机/IO。
4. **序列化规范**：事件用明确字段类型，日志可字节级复现。
5. **可校验哈希**：`logs_hash()` 输出 SHA-256，回放后 `state_hash` 与源一致 → 进 CI 断言。

```
Command::Move{unit,from,to}
   │
   ▼  submit()
+-----------------------------+
| engine: 裁决管线            │
|  plugin[兵种].can_move(ctx) │  ── false → Rejected
|  plugin[裁判].can_move(ctx) │  ── false → Rejected
|  （按注册顺序，全过才固化）  │
+-----------------------------+
   │ 全过
   ▼
Event::MoveAccepted{unit,from,to}   ──▶ logs.push  (append-only)
   │
   ▼
Board::apply(&Event)                ──▶ 世界状态折叠
```

---

## 4. 插件契约（内核 ↔ Lua）

每个 Lua 插件顶层必须是**工厂函数**：

```lua
-- plugin_name.lua
return function(host)          -- host: 内核注入的工具表
  return {
    name = "knight",
    can_move = function(ctx)   -- ctx: 裁决上下文
      -- ctx.from, ctx.to   : 起点/终点 cell（0..63，row*8+col）
      -- ctx.unit           : 走子单位（{id, kind, cell, owner}）
      -- ctx.board          : 占用表（1-indexed：cell+1 → true）
      return true / false
    end,
    -- 未来可扩展：on_attack / on_turn_start / score / describe
  }
end
```

**契约要点**：
- 顶层 `return function(host) … end` 是**工厂** → 每次 `hot_reload` 重新执行，插件可持有自己的初始化状态。
- `name` 作为注册键，同名热插拔原子替换。
- `can_move` 返回 `bool`：这是 M1 的单谓词契约；M2 演进为规则集 DSL 时扩展为 `{effect, conditions}`。
- `host` 预留注入（日志、随机源、时钟、查询 API），当前为空，未来有序数采样等。

---

## 5. 状态与并发模型

- 现有模型：**单线程**。`Rc<PluginRepo>` + `Rc<RefCell<HashMap>>` 共享插件表，引擎持 `Rc` 引用 → 热插拔对引擎透明。
- 只用 `Rc`（非 `Arc`）是因为 M1 明确单线程，避免无关复杂度（如无必要勿增实体）。
- 未来多局并发：`Engine` 实例按局隔离，共享只读的规则集快照；`RefCell` 升级为 `RwLock`/`Arc` 即可，接口不变。

---

## 6. 目录结构

```
wargame/
├── Cargo.toml
├── src/
│   ├── main.rs        # CLI 演示驱动
│   ├── lib.rs         # 模块导出
│   ├── event.rs       # 命令 / 事件类型
│   ├── board.rs       # 棋盘状态 + 纯函数折叠
│   ├── engine.rs      # 裁决管线 + 日志 + 哈希
│   └── host.rs        # Lua 插件宿主（热插拔）
├── plugins/
│   ├── judge.lua      # 裁判：出界/原地/占位
│   └── knight.lua     # 兵种：马走日
├── docs/
│   └── ARCHITECTURE.md # 本文
└── .github/workflows/release.yml  # 云编译三平台发布
```

---

## 7. 里程碑

| 阶段 | 内容 | 状态 |
|---|---|---|
| **M1** | 事件溯源内核 + Lua 规则即插件 + 热插拔 + 确定性哈希 | ✅ 已完成 |
| M2 | 规则集 DSL（数据驱动，仿 FreeCiv ruleset）；对弈协议 | 待 |
| M3 | 全体系对抗建模（司光亚问题：社会域 / 网络域 / 智能化） | 待 |
| M4 | AI / 强化学习接入，多局并发生成 | 待 |

**M1 已验证**（`cargo run` 输出）：
- 合法马步通过，横一格/原地被插件拒绝 ✅
- 热插拔：改 `knight.lua` → `hot_reload` → 内核未重启，规则已生效 ✅
- 回放 `replay == board`，logs/state SHA-256 稳定 ✅

---

*本架构为 Rust 路线基线。M1 代码已落地，后续里程碑在此之上演进。*