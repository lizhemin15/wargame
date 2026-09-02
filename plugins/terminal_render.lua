-- terminal_render.lua —— 终端 ASCII 渲染插件
-- 契约：顶层返回 init(host) 工厂函数，调用后返回 { name=..., render=function(snap) end }
-- 从 snapshot（稳定快照）读棋盘，画成多行 ASCII 字符串返回。
-- 不绑定任何内部结构——只认 snapshot 的固定字段（terrain/units/points/rows/cols）。

-- 兵种 → 显示字符（插件自己的映射，改兵种不改这里）
local DISPLAY = {
  knight = "K", rook = "R", infantry = "I",
  cn_inf = "人", cn_arty = "砲", cn_local = "勇",
  jp_div = "兵", jp_tank = "坦", jp_navy = "舰",
}

return function(host)
  return {
    name = "terminal_render",

    render = function(snap)
      local rows, cols = snap.rows, snap.cols
      -- 地形网格：snap.terrain 是扁平字符串数组（稳定 schema），行优先
      local grid = {}
      for i = 1, rows * cols do grid[i] = snap.terrain[i] end

      -- 覆盖单位
      local units = snap.units
      for i = 1, #units do
        local u = units[i]
        local cell = u.cell
        if cell >= 0 and cell < rows * cols then
          local ch = DISPLAY[u.kind] or string.sub(u.kind, 1, 1)
          -- owner 1 用大写（ASCII 色差），owner 0 用小写
          if u.owner == 1 then ch = string.upper(ch) end
          grid[cell + 1] = ch
        end
      end

      local out = {}
      for r = 0, rows - 1 do
        local line = { tostring(r) .. " " }
        for c = 0, cols - 1 do
          line[#line + 1] = grid[r * cols + c + 1]
          line[#line + 1] = " "
        end
        out[#out + 1] = table.concat(line)
      end
      -- 列标
      local foot = { "   " }
      for c = 0, cols - 1 do foot[#foot + 1] = tostring(c) end
      foot[#foot + 1] = "  (列)"
      out[#out + 1] = table.concat(foot, " ")

      -- 要点状态
      local pts = {}
      for i = 1, #snap.points do
        local p = snap.points[i]
        local own = p.owner
        local mark = own == 0 and "国军" or (own == 1 and "日军" or "未占")
        pts[#pts + 1] = p.name .. "(" .. mark .. ")"
      end
      if #pts > 0 then
        out[#out + 1] = "要点: " .. table.concat(pts, " ")
      end

      out[#out + 1] = "胜者: " .. (snap.winner >= 0 and tostring(snap.winner) or "未分胜负")
      return table.concat(out, "\n")
    end,
  }
end