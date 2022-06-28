use std::io;
use std::future::Future;

use crossbeam_utils::thread;
use mlua::Lua;
use tokio::runtime;

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
    Func: FnOnce(Dispatcher) -> Fut,
    Func: Send,
    Fut: Future,
    Fut::Output: Send,
{
    let (dispatcher, executor) = channel(config.buffer)?;

    let mut fibers = Vec::with_capacity(config.fibers);
    for i in 0..config.fibers {
        let executor = executor.try_clone().unwrap();
        let handle: tarantool::fiber::JoinHandle<i32> = tarantool::fiber::Builder::new()
            .name(format!("xtm{}", i))
            .func(move || loop {
                match executor.exec(lua, config.max_recv_retries, config.coio_timeout) {
                    Ok(_) => continue,
                    Err(ChannelError::TXChannelClosed) => continue,
                    Err(ChannelError::RXChannelClosed) => break 0,
                    Err(_err) => break -1,
                }
            })
            .start()
            .unwrap();

        fibers.push(handle);
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

        for fiber in fibers {
            fiber.join();
        }
        module_thread.join().unwrap().unwrap()
    })
    .unwrap();

    Ok(result)
}
