use std::io;
use std::future::Future;

use crossbeam_utils::thread;
use mlua::Lua;
use tokio::runtime;
pub use txapi::*;

mod eventfd;
mod txapi;
mod fiber_pool;

mod config;
pub use config::*;

use tokio::sync::Notify;
use std::sync::Arc;

pub fn run_module<Fut, Func>(
    module_main: Func,
    config: ModuleConfig,
    lua: &Lua,
    notifier : Arc<Notify>,
) -> io::Result<Fut::Output>
where
    Func: FnOnce(Dispatcher, Arc<Notify>) -> Fut,
    Func: Send,
    Fut: Future,
    Fut::Output: Send,
{
    let (dispatcher, executor) = channel(config.buffer)?;

    let mut fiber_pool = fiber_pool::FiberPool::new(lua, executor, config.clone());
    fiber_pool.run()?;

    let result = thread::scope(|scope| {
        let module_thread = scope
            .builder()
            .name("module".to_owned())
            .spawn(move |_| -> io::Result<Fut::Output> {
                let rt = runtime::Builder::from(config.runtime)
                    .enable_io()
                    .enable_time()
                    .build()?;

                Ok(rt.block_on(module_main(dispatcher, notifier)))
            })
            .unwrap();

        fiber_pool.join();
        module_thread.join().unwrap().unwrap()
    })
    .unwrap();

    Ok(result)
}
