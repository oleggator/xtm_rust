use std::io;
use std::{convert::TryFrom, future::Future};

use crossbeam_utils::thread;
use mlua::Lua;
use tokio::runtime;

pub use txapi::*;
use tarantool::fiber::Fiber;

mod eventfd;
mod txapi;

mod config;
pub use config::*;

pub fn run_module<Fut, Func>(
    module_main: Func,
    config: ModuleConfig,
    lua: &Lua,
) -> io::Result<Fut::Output>
where
    Func: FnOnce(AsyncDispatcher) -> Fut,
    Func: Send,
    Fut: Future,
    Fut::Output: Send,
{
    let (dispatcher, executor) = channel(config.buffer)?;
    let executor_loop = &mut |args: Box<(&Lua, Executor)>| {
        let (lua, executor) = *args;
        while executor.exec(lua).is_ok() {}
        0
    };

    let result = thread::scope(|scope| {
        let module_thread = scope
            .builder()
            .name("module".to_owned())
            .spawn(move |_| -> io::Result<Fut::Output> {
                let rt = runtime::Builder::from(config.runtime)
                    .enable_io()
                    .enable_time()
                    .build()?;

                rt.block_on(async move {
                    let async_dispather = AsyncDispatcher::try_from(dispatcher)?;
                    Ok(module_main(async_dispather).await)
                })
            })
            .unwrap();

        {
            let mut fiber = Fiber::new("xtm", executor_loop);
            fiber.set_joinable(true);
            fiber.start((lua, executor));
            fiber.join();
        }

        module_thread.join().unwrap().unwrap()
    })
    .unwrap();

    Ok(result)
}
