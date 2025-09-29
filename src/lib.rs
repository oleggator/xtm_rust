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
    CreateFiber: Fn(Executor<T>) -> tarantool::fiber::JoinHandle<'a, ()>,
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
        // yield to start fibers
        tarantool::fiber::sleep(std::time::Duration::ZERO);

        for fiber in fibers {
            fiber.join();
        }
        module_thread.join().unwrap()
    })
    .unwrap()
}

unsafe extern "C" {
    unsafe fn luaT_state() -> *mut mlua::lua_State;
}

unsafe fn mlua_state<'a>() -> &'a mlua::Lua {
    unsafe { mlua::Lua::get_or_init_from_ptr(luaT_state().cast()) }
}

fn create_fiber(
    _lua: &Lua,
    max_recv_retries: usize,
    coio_timeout: f64,
    executor: Executor<Lua>,
) -> tarantool::fiber::JoinHandle<'static, ()> {
    let func = move || {
        let lua = unsafe { mlua_state() };
        loop {
            match executor.exec(lua, max_recv_retries, coio_timeout) {
                Ok(()) | Err(ExecError::ResultChannelSendError) => continue,
                Err(ExecError::TaskChannelRecvError) => break,
            }
        }
    };

    Fyber::spawn_lua("xtm".to_owned(), func, None).unwrap()
}
