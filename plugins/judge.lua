-- judge.lua —— 裁判插件（通用规则裁决）
-- 契约：顶层返回 init(host) 工厂函数，调用后返回 { name=..., can_move=function(ctx) end }
-- 演示"规则即插件"：裁判与兵种走法分离，可独立热插拔。
-- 职责：除了"能按几何走"，还裁决"能不能走"（不出界、不原地、不落入敌方已占格）。

return function(host)
  return {
    name = "judge",

    -- ctx: { from, to, unit, board }  board 是 0..63 扁平编码的占用表（值为 true 表示被占）
    can_move = function(ctx)
      local to, unit = ctx.to, ctx.unit
      local board = ctx.board

      -- 1. 不出界（to 永远在 0..63，核心里已保证，双保险）
      if to < 0 or to > 63 then return false end

      -- 2. 不原地移动 / 不移动到自身所在格
      if to == ctx.from then return false end

      -- 3. 不落入已被占用的格子（禁入，载入逻辑里不允许吃子）
      if board[to + 1] == true then
        return false -- 目标格被占（不论敌我），M1 阶段禁吃子
      end

      return true
    end
  }
end