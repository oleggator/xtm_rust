mod grpc;

use mlua::prelude::*;
use xtm_rust::{run_module, MluaExecutorLoopConfig, ModuleConfig};

#[mlua::lua_module]
fn grpc(lua: &'static Lua) -> LuaResult<LuaTable> {
    let exports = lua.create_table()?;

    exports.set(
        "start",
        lua.create_function_mut(|lua, (config,): (LuaValue,)| {
            let config: ModuleConfig = lua.from_value(config)?;

            run_module(
                grpc::module_main,
                config,
                lua,
                MluaExecutorLoopConfig {
                    coio_timeout: config.coio_timeout,
                    max_recv_retries: config.max_recv_retries,
                },
            )
            .map_err(LuaError::external)
        })?,
    )?;

    Ok(exports)
}
