//! Lua 插件宿主：热插拔 + 统一插件契约
//!
//! 契约（内核 ↔ Lua 插件）：
//!   Lua 文件顶层定义 `init(host) -> plugin_table`
//!   返回的 plugin_table 必须含字段 `name` 与函数 `can_move(ctx)`
//!   ctx 是内核注入给 Lua 的表，含 from / to / unit / board 视图

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use mlua::{Lua, Table};

use crate::board::Board;
use crate::event::{Cell, Unit};

/// 运行时插件句柄：包住一个已初始化的插件表
pub struct Plugin {
    pub name: String,
    pub lua_rc: Rc<Lua>,
    pub can_move: Box<dyn Fn(&Unit, Cell, Cell, &Board) -> bool>,
}

impl Plugin {
    fn from_table(lua_rc: Rc<Lua>, t: &Table, plugin_name: &str) -> Result<Plugin, mlua::Error> {
        let can_move_fn = t.get::<mlua::Function>("can_move")?;
        let lua = lua_rc.clone();
        let can_move: Box<dyn Fn(&Unit, Cell, Cell, &Board) -> bool> =
            Box::new(move |unit, from, to, board| {
                let ctx = lua.create_table().expect("ctx table");
                let unit_t = lua.create_table().expect("unit table");
                let _ = unit_t.set("id", unit.id);
                let _ = unit_t.set("kind", unit.kind.clone());
                let _ = unit_t.set("cell", unit.cell);
                let _ = unit_t.set("owner", unit.owner);
                let _ = ctx.set("unit", unit_t);
                let _ = ctx.set("from", from);
                let _ = ctx.set("to", to);
                // 占位视图：目标格是否被占（1-indexed Lua 数组风格）
                let occ_t = lua.create_table().expect("occ table");
                for (c, _uid) in board.occ.iter() {
                    let _ = occ_t.set(*c + 1, true);
                }
                let _ = ctx.set("board", occ_t);
                can_move_fn.call::<bool>(ctx).unwrap_or(false)
            });
        Ok(Plugin {
            name: plugin_name.to_string(),
            lua_rc,
            can_move,
        })
    }
}

/// 插件仓库：名 → 插件句柄。热插拔 = 原子替换这里的条目。
/// 用 RefCell 包裹以便多持方（Engine / main）共享可变访问。
pub struct PluginRepo {
    dir: PathBuf,
    plugins: RefCell<HashMap<String, Rc<Plugin>>>,
}

impl PluginRepo {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            plugins: RefCell::new(HashMap::new()),
        }
    }

    /// 从 Lua 源码加载并注册插件（返回插件名）。
    fn load_src(&self, src: &str, tag: &str) -> mlua::Result<String> {
        let lua = Rc::new(Lua::new());
        let init_fn: mlua::Function = lua.load(src).set_name(tag).eval().map_err(|e| {
            mlua::Error::RuntimeError(format!("eval {}: {}", tag, e))
        })?;
        let host = lua.create_table()?;
        let plugin_t: Table = init_fn.call(host)?;
        let name = plugin_t.get::<String>("name")?;
        let p = Plugin::from_table(lua.clone(), &plugin_t, &name)?;
        self.plugins.borrow_mut().insert(name.clone(), Rc::new(p));
        Ok(name)
    }

    /// 加载单个插件文件并注册（返回插件名）。
    pub fn load(&self, file: &str) -> mlua::Result<String> {
        let path = self.dir.join(file);
        let src = std::fs::read_to_string(&path)
            .map_err(|e| mlua::Error::RuntimeError(format!("read {}: {}", path.display(), e)))?;
        self.load_src(&src, file)
    }

    /// 热重载：重新读文件 + init + 原子替换。
    pub fn hot_reload(&self, name: &str) -> mlua::Result<bool> {
        let file = format!("{}.lua", name);
        let path = self.dir.join(&file);
        let src = std::fs::read_to_string(&path)
            .map_err(|e| mlua::Error::RuntimeError(format!("read {}: {}", path.display(), e)))?;
        let lua = Rc::new(Lua::new());
        let init_fn: mlua::Function = lua.load(&src).set_name(&file).eval().map_err(|e| {
            mlua::Error::RuntimeError(format!("eval {}: {}", file, e))
        })?;
        let host = lua.create_table()?;
        let plugin_t: Table = init_fn.call(host)?;
        let loaded_name = plugin_t.get::<String>("name")?;
        if loaded_name != name {
            return Err(mlua::Error::RuntimeError(format!(
                "plugin name mismatch: {} vs {}",
                name, loaded_name
            )));
        }
        let p = Plugin::from_table(lua, &plugin_t, name)?;
        self.plugins.borrow_mut().insert(name.to_string(), Rc::new(p)); // 原子替换
        Ok(true)
    }

    pub fn get(&self, name: &str) -> Option<Rc<Plugin>> {
        self.plugins.borrow().get(name).cloned()
    }

    pub fn names(&self) -> Vec<String> {
        self.plugins.borrow().keys().cloned().collect()
    }
}