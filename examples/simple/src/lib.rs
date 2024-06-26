use mlua::prelude::*;
use serde::{Deserialize, Serialize};
use tarantool::space::Space;
use tarantool::tuple::Encode;
use xtm_rust::Dispatcher;
use xtm_rust::{run_module_with_mlua, ModuleConfig};

#[derive(Serialize, Deserialize)]
struct Row {
    pub int_field: i32,
    pub str_field: String,
}

impl Encode for Row {}

async fn module_main(dispatcher: Dispatcher<Lua>) {
    let result = dispatcher
        .call(move |_| {
            let space = Space::find("some_space").unwrap();
            let _result = space
                .replace(&Row {
                    int_field: 1,
                    str_field: "inserted from module using rust".to_owned(),
                })
                .unwrap();

            100
        })
        .await
        .unwrap();
    assert_eq!(result, 100);

    dispatcher
        .call(move |lua| {
            lua.load(
                "
                box.space.some_space:insert {
                    2,
                    'inserted from module using lua',
                }
            ",
            )
            .exec()
            .unwrap();
        })
        .await
        .unwrap();
}

#[mlua::lua_module]
fn simple(lua: &Lua) -> LuaResult<LuaTable> {
    let exports = lua.create_table()?;

    exports.set(
        "start",
        lua.create_function_mut(|lua, (config,): (LuaValue,)| {
            let config: ModuleConfig = lua.from_value(config)?;
            run_module_with_mlua(module_main, config, lua).map_err(LuaError::external)
        })?,
    )?;

    Ok(exports)
}
