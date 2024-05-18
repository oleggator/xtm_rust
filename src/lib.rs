mod config;
mod notify;
mod txapi;

use std::future::Future;
use std::io;

use crossbeam_utils::thread;
use mlua::Lua;
use tarantool::fiber::Fyber;
use tokio::runtime;

pub use config::*;
pub use txapi::*;

pub fn run_module_with_mlua<Fut, Func>(
    module_main: Func,
    config: ModuleConfig,
    lua: &Lua,
) -> io::Result<Fut::Output>
where
    Func: Send + FnOnce(Dispatcher<Lua>) -> Fut,
    Fut: Future,
    Fut::Output: Send,
{
    let max_recv_retries = config.max_recv_retries;
    let coio_timeout = config.coio_timeout;
    run_module(module_main, config, |executor: Executor<Lua>| {
        create_fiber(lua, max_recv_retries, coio_timeout, executor)
    })
}

pub fn run_module<'a, Fut, Func, CreateFiber, T>(
    module_main: Func,
    config: ModuleConfig,
    create_fiber: CreateFiber,
) -> io::Result<Fut::Output>
where
    Func: Send + FnOnce(Dispatcher<T>) -> Fut,
    Fut: Future,
    Fut::Output: Send,
    CreateFiber: Fn(Executor<T>) -> tarantool::fiber::JoinHandle<'a, Result<i32, mlua::Error>>,
{
    let (dispatcher, executor) = channel(config.buffer)?;

    let mut fibers = Vec::with_capacity(config.fibers);
    for _ in 0..config.fibers {
        let executor = executor.try_clone()?;
        let fiber = create_fiber(executor);
        fibers.push(fiber);
    }

    thread::scope(|scope| {
        let module_thread = scope.builder().name("module".to_owned()).spawn(
            move |_| -> io::Result<Fut::Output> {
                let rt = runtime::Builder::from(config.runtime)
                    .enable_io()
                    .enable_time()
                    .build()?;

                Ok(rt.block_on(module_main(dispatcher)))
            },
        )?;
        tarantool::fiber::sleep(std::time::Duration::ZERO);

        for fiber in fibers {
            let _result = fiber.join().unwrap();
        }
        module_thread.join().unwrap()
    })
    .unwrap()
}

fn create_fiber(
    lua: &Lua,
    max_recv_retries: usize,
    coio_timeout: f64,
    executor: Executor<Lua>,
) -> tarantool::fiber::JoinHandle<'_, Result<i32, mlua::Error>> {
    let thread_func = move |lua, ()| loop {
        match executor.exec(lua, max_recv_retries, coio_timeout) {
            Ok(()) | Err(ExecError::ResultChannelSendError) => continue,
            Err(ExecError::TaskChannelRecvError) => break Ok(0),
        }
    };
    let thread_func = lua.create_function(thread_func).unwrap();
    let thread = lua.create_thread(thread_func).unwrap();

    Fyber::spawn_deferred(
        "xtm".to_owned(),
        move || -> std::result::Result<i32, mlua::Error> { thread.resume(()) },
        true,
        None,
    )
    .unwrap()
    .unwrap()
}
