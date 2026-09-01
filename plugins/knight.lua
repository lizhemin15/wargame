-- knight.lua —— 马兵种插件（热插拔版本2：斜走一格）
return function(host)
  return {
    name = "knight",
    can_move = function(ctx)
      local from, to = ctx.from, ctx.to
      local dr = math.abs((from // 8) - (to // 8))
      local dc = math.abs((from % 8) - (to % 8))
      return (dr == 1 and dc == 1)
    end
  }
end
