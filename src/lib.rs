use std::future::Future;
use std::io;

use crossbeam_utils::thread;
use mlua::Lua;
use tokio::runtime;

use tarantool::fiber::Fiber;
pub use txapi::*;

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
    Func: FnOnce(Dispatcher<Lua>) -> Fut,
    Func: Send,
    Fut: Future,
    Fut::Output: Send,
{
    let (dispatcher, executor) = channel(config.buffer)?;

    let executor_loop = &mut |args: Box<(&Lua, Executor<Lua>)>| {
        let (lua, executor) = *args;

        let thread_func = lua
            .create_function(move |lua, _: ()| {
                Ok(loop {
                    match executor.exec(lua, config.max_recv_retries, config.coio_timeout) {
                        Ok(_) => continue,
                        Err(ChannelError::TXChannelClosed) => continue,
                        Err(ChannelError::RXChannelClosed) => break 0,
                        Err(_err) => break -1,
                    }
                })
            })
            .unwrap();
        let thread = lua.create_thread(thread_func).unwrap();
        thread.resume(()).unwrap()
    };

    // UNSAFE: fibers must die inside the current function
    let mut fibers = Vec::with_capacity(config.fibers);
    for _ in 0..config.fibers {
        let mut fiber = Fiber::new("xtm", executor_loop);
        fiber.set_joinable(true);
        fiber.start((lua, executor.try_clone()?));
        fibers.push(fiber);
    }

    let result = thread::scope(|scope| {
        let module_thread = scope
            .builder()
            .name("module".to_owned())
            .spawn(move |_| -> io::Result<Fut::Output> {
                let rt = runtime::Builder::from(config.runtime)
                    .enable_io()
                    .enable_time()
                    .build()?;

                Ok(rt.block_on(module_main(dispatcher)))
            })
            .unwrap();

        for fiber in &fibers {
            fiber.join();
        }
        module_thread.join().unwrap().unwrap()
    })
    .unwrap();

    Ok(result)
}
