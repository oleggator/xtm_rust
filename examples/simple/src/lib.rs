use mlua::prelude::*;
use xtm_rust::AsyncDispatcher;
use tokio::time::Instant;
use serde::{Deserialize, Serialize};
use tarantool::space::Space;
use tarantool::tuple::AsTuple;
use futures::future::join_all;
use xtm_rust::run_module;

#[derive(Serialize, Deserialize)]
struct Row {
    pub int_field: i32,
    pub str_field: String,
}

impl AsTuple for Row {}

async fn module_main(dispatcher: AsyncDispatcher) {
    let iterations = 1_000_000;

    let workers = 8;
    let iterations_per_worker = iterations / workers;
    let mut futures = Vec::new();


    let begin = Instant::now();
    for i in 1..workers {
        let dispatcher = dispatcher.try_clone().unwrap();
        let fut = tokio::spawn(async move {
            for j in i*iterations_per_worker..(i+1)*iterations_per_worker {
                let result = dispatcher.call(move |_| {
                    let mut space = Space::find("some_space").unwrap();
                    let _result = space.replace(&Row {
                        int_field: j as i32,
                        str_field: "some_string".to_owned(),
                    }).unwrap();

                    100
                }).await.unwrap();
                assert_eq!(result, 100);
            }
        });
        futures.push(fut);
    }
    join_all(futures).await;

    let elapsed = begin.elapsed().as_nanos();
    let avg = elapsed / iterations;
    println!("iterations: {} | elapsed: {}ns | avg per cycle: {}ns | avg rps: {}",
             iterations, elapsed, avg, iterations * 1_000_000_000 / elapsed);
}

#[mlua::lua_module]
fn libtxapi(lua: &Lua) -> LuaResult<LuaTable> {
    let exports = lua.create_table()?;

    exports.set("start", lua.create_function_mut(|lua, (buffer, )| {
        run_module(buffer, module_main, lua).unwrap();
        Ok(())
    })?)?;

    Ok(exports)
}
