-- to_json.lua —— JSON 结构化渲染插件
-- 契约：顶层返回 init(host) 工厂函数，返回 { name=..., render=function(snap) end }
-- 从 snapshot（稳定快照）提取一份紧凑的 JSON 视图返回。
-- 内核负责把返回值转成 JSON（mlua serde）；插件只负责"选什么数据 + 怎么组织"。

return function(host)
  return {
    name = "to_json",

    render = function(snap)
      -- 紧凑视图：只留规则/坐标/存活单位清单/归属要点/胜负
      local view = {
        ruleset = snap.ruleset_name,
        size = { rows = snap.rows, cols = snap.cols },
        winner = snap.winner,
        units = {},
        objectives = {},
      }
      for i = 1, #snap.units do
        local u = snap.units[i]
        if u.hp > 0 then
          view.units[#view.units + 1] = {
            id = u.id, kind = u.kind, owner = u.owner,
            cell = u.cell, hp = u.hp,
          }
        end
      end
      for i = 1, #snap.points do
        local p = snap.points[i]
        view.objectives[#view.objectives + 1] = {
          name = p.name, cell = p.cell, owner = p.owner,
        }
      end
      return view
    end,
  }
end