use mlua::prelude::*;
use xtm_rust::txapi::{tx_channel, Executor, Dispatcher, ExecError};
use tokio::runtime;
use tokio::time::Instant;


type R = i32;
type F = Box<dyn FnOnce() -> R + Send + 'static>;

async fn module_main(dispatcher: Dispatcher<F, R>) {
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

fn tx_main(executor: Box<Executor<F, R>>) -> i32 {
    loop {
        match executor.exec() {
            Ok(_) => {}
            Err(ExecError::RXChannelClosed) => {
                break 0;
            }
            Err(err) => {
                println!("{:#?}", err);
                break 1;
            }
        }
    }
}

#[mlua::lua_module]
fn libtxapi(lua: &Lua) -> LuaResult<LuaTable> {
    let exports = lua.create_table()?;

    exports.set("start", lua.create_function_mut(|_: &Lua, (buffer, ): (usize, )| {
        let (dispatcher, executor) = tx_channel(buffer);

        let module_thread = std::thread::Builder::new()
            .name("module".to_string())
            .spawn(move || {
                runtime::Builder::new_current_thread()
                    .enable_io()
                    .build()
                    .unwrap()
                    .block_on(module_main(dispatcher))
            })
            .unwrap();

        let mut tx_fiber = tarantool::fiber::Fiber::new("module_tx", &mut tx_main);
        tx_fiber.set_joinable(true);
        tx_fiber.start(executor);

        tx_fiber.join();
        module_thread.join().unwrap();
        Ok(())
    })?)?;

    Ok(exports)
}
