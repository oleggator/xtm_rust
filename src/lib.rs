use std::convert::TryInto;
use std::io;
use std::{convert::TryFrom, future::Future};

use crossbeam_utils::thread;
use tokio::runtime;

use tarantool::fiber::Fiber;
pub use txapi::*;

mod eventfd;
mod txapi;

mod config;
pub use config::*;

type ExecutorLoop<T> = Box<dyn FnMut(Box<(T, Executor<T>)>) -> i32>;

pub trait ExecutorLoopMaker {
    type LoopConfig: Copy;

    fn make_loop_maker(&self, config: Self::LoopConfig) -> ExecutorLoop<&'static Self>;
}


#[derive(Clone, Copy)]
pub struct MluaExecutorLoopConfig {
    pub max_recv_retries: usize,
    pub coio_timeout: f64,
}

impl ExecutorLoopMaker for mlua::Lua {
    type LoopConfig = MluaExecutorLoopConfig;

    fn make_loop_maker(&self, config: Self::LoopConfig) -> ExecutorLoop<&'static Self> {
        Box::new(move |args: Box<(&mlua::Lua, Executor<&mlua::Lua>)>| {
            let (lua, executor) = *args;

            let thread_func = lua
                .create_function(move |lua, _: ()| {
                    Ok(loop {
                        match executor.exec(lua, config.max_recv_retries, config.coio_timeout) {
                            Ok(_) => continue,
                            Err(ChannelError::RXChannelClosed) => break 0,
                            Err(_err) => break -1,
                        }
                    })
                })
                .unwrap();
            let thread = lua.create_thread(thread_func).unwrap();
            thread.resume(()).unwrap()
        })
    }
}

pub fn run_module<Fut, Func, T>(
    module_main: Func,
    config: ModuleConfig,
    elm: &'static T,
    loop_config: T::LoopConfig,
) -> io::Result<Fut::Output>
where
    Func: FnOnce(AsyncDispatcher<&'static T>) -> Fut,
    Func: Send,
    Fut: Future,
    Fut::Output: Send,
    T: ExecutorLoopMaker + 'static,
{
    let (dispatcher, executor) = channel(config.buffer)?;

    let mut executor_loop = elm.make_loop_maker(loop_config);

    // UNSAFE: fibers must die inside the current function
    let mut fibers = Vec::with_capacity(config.fibers);
    for _ in 0..config.fibers {
        let mut fiber = Fiber::new("xtm", &mut executor_loop);
        fiber.set_joinable(true);
        fiber.start((&elm, executor.try_clone()?));
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

                rt.block_on(async move {
                    let async_dispather = AsyncDispatcher::try_from(dispatcher)?;
                    Ok(module_main(async_dispather).await)
                })
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
