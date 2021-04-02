use mlua::prelude::*;
use xtm_rust::txapi::{Dispatcher, run_module};
use tokio::time::Instant;

async fn module_main(dispatcher: Dispatcher<Box<dyn FnOnce() -> i32 + Send + 'static>, i32>) {
    let iterations = 1_000_000;

    let now = Instant::now();
    for _ in 0..iterations {
        let result = dispatcher.call(Box::new(move || {
            100
        })).await.unwrap();
        assert_eq!(result, 100);
    }

    let elapsed = now.elapsed().as_nanos();
    let avg = elapsed / iterations;
    println!("iterations: {} | elapsed: {}ns | avg per cycle: {}ns", iterations, elapsed, avg);
}

#[mlua::lua_module]
fn libtxapi(lua: &Lua) -> LuaResult<LuaTable> {
    let exports = lua.create_table()?;

    exports.set("start", lua.create_function_mut(|_: &Lua, (buffer, ): (usize, )| {
        run_module(buffer, module_main);
        Ok(())
    })?)?;

    Ok(exports)
}
