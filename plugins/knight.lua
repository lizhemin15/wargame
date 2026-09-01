-- knight.lua —— 马兵种插件（马走日）
-- 契约：顶层返回 init(host) 工厂函数，返回 { name=..., can_move=function(ctx) end }
-- 本文件演示"兵种 = 插件"：改走法规则只需改这个文件并热重载，Rust 内核零改动。

return function(host)
  return {
    name = "knight",
    can_move = function(ctx)
      local from, to = ctx.from, ctx.to
      local dr = math.abs((from // 8) - (to // 8))
      local dc = math.abs((from % 8) - (to % 8))
      -- 马走日：一个方向差 2，另一个方向差 1（不区分横向/纵向）
      return (dr == 1 and dc == 2) or (dr == 2 and dc == 1)
    end
  }
end