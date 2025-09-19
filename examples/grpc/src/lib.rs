mod grpc;

use mlua::prelude::*;
use xtm_rust::{ModuleConfig, run_module_with_mlua};

#[mlua::lua_module]
fn grpc(lua: &Lua) -> LuaResult<LuaTable<'_>> {
    let exports = lua.create_table()?;

    exports.set(
        "start",
        lua.create_function_mut(|lua, (config,): (LuaValue,)| {
            let config: ModuleConfig = lua.from_value(config)?;
            run_module_with_mlua(grpc::module_main, config, lua).map_err(LuaError::external)
        })?,
    )?;

    Ok(exports)
}
