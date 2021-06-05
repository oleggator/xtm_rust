mod grpc;

use mlua::prelude::*;
use xtm_rust::run_module;

#[mlua::lua_module]
fn grpc(lua: &Lua) -> LuaResult<LuaTable> {
    let exports = lua.create_table()?;

    exports.set(
        "start",
        lua.create_function_mut(|lua, (buffer,)| {
            run_module(buffer, grpc::module_main, lua)
                .map_err(LuaError::external)
        })?,
    )?;

    Ok(exports)
}
