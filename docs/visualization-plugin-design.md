# 可视化插件契约设计草案（可视化 = 插件）

> 状态：**草案待评审** —— 未动任何代码，先把契约谈清楚。
> 原则：与「兵种 = 插件」同源。**呈现方式由插件说了算，Rust 内核只负责裁决与状态，不负责"长什么样"。**

---

## 0. 想解决的问题

现在 `main.rs` 里 `render()` 是**硬编码**的：

```rust
// src/main.rs:236
let ch = match u.kind.as_str() {
    "knight" => '♞', "rook" => '♜', "cn_inf" => '人', "jp_div" => '兵', ...
    _ => u.kind.chars().next().unwrap_or('?'),
};
```

- 兵种→字符映射写死在 Rust。
- 渲染方式（终端 ASCII）写死在 Rust。
- 一旦要加 Web 页面 / SVG / 结构化 JSON，又得往内核塞渲染代码 —— 与"内核只裁决"的架构原则冲突。

**目标**：把"怎么呈现棋盘"抽出来，做成 Lua 插件。要终端就装 terminal 插件，要网页就装 web 插件，要 JSON 就装 json 插件。内核零改动。

---

## 1. 核心思路：插件输出「结构化快照」而非「画面本身」

**分歧点在这** —— 我建议契约是：

> **插件收到一个完整的棋盘快照（ctx），插件负责把它转换成一个可 JSON 序化的值返回。内核不关心它画了什么，只把这个值拿走去打印 / 导出 / 交给前端。**

为什么不直接让 Lua 插件 `print` 字符串？

| 方案 | 优点 | 缺点 |
|---|---|---|
| **A. 插件返回结构化值**（JSON/表） | 与渲染方式解耦；终端/web/SVG 共用同一数据面；`.lua` 无副作用、确定性好、可测试 | 插件里"纯 Lua 画 ANSI 字符串"略绕一步 |
| **B. 插件直接 print 字符串** | 简单直接，终端省事 | 绑死终端；web 用不上；引入副作用破坏确定性回放哲学 |

> 结论倾向 **A（结构化值）**，同时允许返回值本身是字符串（终端插件可以返回一段多行 ANSI 文本）。这样 A、B 都能满足，插件自己选。

---

## 2. 数据面：注入给插件的 ctx

参考 `Board` / `Unit` / `Ruleset` 的实际字段（`src/board.rs`、`src/event.rs`、`src/ruleset.rs`），ctx 设计如下：

```lua
ctx = {
  -- 规则集元信息
  ruleset = {
    name = "淞沪会战",
    rows = 15, cols = 21,
  },

  -- 完整地形：flatten 的每格符号，长度 = rows*cols，按 rowwise 排列
  terrain = { "~","~","~",...,".","F",... },

  -- 所有存活单位（Board.units 过滤 hp>0）
  units = {
    { id=1, kind="cn_inf", name="国民革命军第88师", owner=0, cell=269, hp=1, moves=2 },
    { id=6, kind="jp_div", name="日军第3师团",      owner=1, cell=38,  hp=1, moves=2 },
    ...
  },

  -- 要点 + 当前占领方
  points = {
    { id=0, name="罗店",   cell=152, owner=-1 },   -- -1/未占领
    { id=1, name="四行仓库",cell=247, owner=0 },
    ...
  },

  -- 胜负（GameOver 事件后才有值）
  victory = { winner=0, reason="need_points 达标" },

  -- 事件日志（已折叠进 board 的所有事件，可回放）
  logs = { "MoveAccepted{unit=1,from=269,to=248}", "PointTaken{point=1,owner=0}", ... },

  -- 服务器/宿主给插件的辅助（想要再加）
  host = { ... },
}
```

> 注：`name` 字段当前 Unit 没有，ruleset 也没存赢点名 —— 需要在 ruleset/Unit 上**补一个展示名**（见 §5 待办）。cell 用扁平索引（`row*cols+col`），与全引擎一致。

---

## 3. 接口关系：与 can_move 插件并存

现契约（`src/host.rs:5-6`）：
```
Lua 文件顶层 init(host) -> plugin_table，必须含 name 与 can_move(ctx)
```

**建议：可视化是"普通插件多一个可选钩子"，不是独立一类。** 理由：

- 一个 `init` 返回 `{ name, can_move?, render?, ... }` —— **钩子是可选字段**，插件按需实现。
- 现在 loader（`Plugin::from_table`，`host.rs:26`）**强依赖 can_move**（`t.get::<Function>("can_move")?` 取不到就 Err）。
- 要让可视化插件能单独存在，必须把 can_move 从「必填」改成「可选」，get 不到就留 `None`。

```lua
-- plugins/terminal_render.lua
return function(host)
  return {
    name = "terminal_render",       -- 仅渲染，不裁决
    render = function(ctx)
      -- ... 把 ctx 画成多行 ANSI 字符串返回
      return lines
    end
  }
end
```

```lua
-- plugins/blue_unit.lua  （仍可同时裁决）
return function(host)
  return {
    name = "blue",
    can_move = function(ctx) ... end,
    render = function(ctx) ... end,   -- 可选，一个插件可两职责
  }
end
```

### 触发模型：内核在 `finish()` 后调用 render

不是"问"（如 can_move 在裁决中被调），而是**终局后主动调用一次**：

```
main: 推演结束 -> 组装 ctx 快照 -> for 每个注册的插件，若有 render(ctx) 则调用
      -> 拿返回值 -> 打印（或写入 rulesets/<name>-render.json / 输出给前端）
```

同时可加命令行选项选渲染插件：`--render json` / `--render terminal`。

---

## 4. 落地的三步（先不写码，先对齐）

### Step 1 — host 改造：can_move 变可选
- `Plugin` 结构体加 `render: Option<Box<dyn Fn(&SnapshotCtx) -> mlua::Value>>`。
- `from_table` 里 can_move 用 `t.get` 包裹，None 也合法。
- 新增 `SnapshotCtx`，把 board+ruleset+events 组装好（§2）。
- 热重载 `hot_reload` 同样适配。

### Step 2 — 内置插件示例
- `plugins/terminal_render.lua`：把现在的硬编码 render 逻辑下沉成 Lua（地形 symbol + 兵种字符映射 + owner 大小写）。**验证：跑淞沪/百团输出的终端图与现在一致**（golden 比对）。
- `plugins/to_json.lua`：把 ctx 原样序列化成 JSON 字符串返回。

### Step 3 — main 接线
- 终局后根据 `--render <插件名>` 调用对应插件，打印返回。
- `--render json` 把棋盘导出成 JSON（为后续 Web 前端铺路）。

---

## 5. 配套小改（依赖项）

1. **Unit / deploy 补 `name`（显示名）** —— 现在 Unit 只有 kind/short 名，ctx 里"88师/第3师团"这类可读名没有来源。ruleset deploy 已给 name（`rulesets/songhu.toml` 有），但 Unit 没带进 board。**要么 Unit 加 name 字段**（折叠状态自包含），要么 ruleset 里查表。倾向 **Unit 加 name**（符合状态自包含原则）。
2. **接口确定性**：render 插件应保持纯函数（只读 ctx），避免写全局/随机 —— 契合事件溯源哲学，也让 render 可 golden 测试。

---

## 6. 一个更想确认的点：渲染方式要不要也"插件写死"？

上面把**数据快照**插件化是明确的。但你原话是"**到底怎么呈现，插件说了算**" —— 这可能有两层意思：

- **(a) 呈现的"数据组织"插件说了算**（我上面的方案）：插件决定吐 JSON 还是 ASCII，但最终"人类看到的画面"仍由内核/前端再加工。
- **(b) 呈现的"最终画面"插件说了算**：插件直接吐出完整的、人类直接看的画面（如完整 HTML 页面 / SVG / ANSI 画布），内核完全不碰视觉。

两者差异在于：**内核要不要参与"最终视觉渲染"**。如果你要的是 (b) 极端版，那插件可能得能返回/写出 HTML 文件，甚至插件自己起服务 —— 这就牵扯"插件能有哪些宿主能力"（文件写、网络），架构复杂度上一个台阶。

---

## 7. 待你拍板的问题

1. **§6 的 (a) 还是 (b)？** —— 内核是否参与最终视觉呈现。我默认 (a)。
2. **可视化插件是否确实和 can_move 共用 `init` 契约**（§3，can_move 变可选），还是要独立 `render_init` 通道？
3. **ctx 数据面（§2）够不够**？要不要加：单位剩余移动力、当前行动方(turn)、地形逐格的可通行性提示？
4. **`--render` 优先级**：命令行指定 > 默认 terminal？还是 ruleset 里配 `[render] plugin = "..."` 数据驱动指定？
5. **Unit 补 name 字段**（§5.1）认可吗？

定完这几点我再动手。当前**一行代码没改**。