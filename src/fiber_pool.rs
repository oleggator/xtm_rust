use std::{collections::LinkedList, rc::Rc};

use crossbeam_channel::{unbounded, TryRecvError};
use mlua::Lua;
use tarantool::fiber;

use crate::{ChannelError, Executor, ModuleConfig, Task};

struct SchedulerArgs<'a> {
    lua: &'a Lua,
    executor: Executor,
    config: ModuleConfig,
}
fn scheduler_f(args: Box<SchedulerArgs>) -> i32 {
    let SchedulerArgs {
        lua,
        executor,
        config,
    } = *args;
    let ModuleConfig {
        max_batch,
        coio_timeout,
        fibers,
        ..
    } = config;

    let cond = Rc::new(fiber::Cond::new());
    let (tx, rx) = unbounded::<Task>();

    let mut workers = LinkedList::new();
    for _ in 0..fibers {
        let mut worker = fiber::Fiber::new("worker", &mut worker_f);
        worker.start(WorkerArgs {
            cond: cond.clone(),
            lua,
            rx: rx.clone(),
        });
        workers.push_back(worker);
    }

    loop {
        let tasks = match executor.pop_many(max_batch, coio_timeout) {
            Ok(tasks) => tasks,
            Err(ChannelError::TXChannelClosed) => break,
            Err(ChannelError::RXChannelClosed) => return 0,
            Err(_err) => return -1,
        };

        for task in tasks {
            tx.send(task).unwrap();
            cond.signal();
        }
    }

    for worker in workers {
        worker.join();
    }

    0
}

struct WorkerArgs<'a> {
    cond: Rc<fiber::Cond>,
    lua: &'a Lua,
    rx: crossbeam_channel::Receiver<Task>,
}
fn worker_f(args: Box<WorkerArgs>) -> i32 {
    let WorkerArgs { cond, lua, rx } = *args;

    let thread_func = lua
        .create_function(move |lua, _: ()| {
            loop {
                match rx.try_recv() {
                    Ok(task) => task(lua),
                    Err(TryRecvError::Disconnected) => return Ok(()),
                    Err(TryRecvError::Empty) => {
                        cond.wait();
                        // let ok = cond.wait_timeout(std::time::Duration::from_secs(1));
                        // if !ok {
                            // kill fiber
                            // return Ok(());
                        // }
                        Ok(())
                    }
                }
                .unwrap();
            }
        })
        .unwrap();
    let thread = lua.create_thread(thread_func.clone()).unwrap();
    let _: () = thread.resume(()).unwrap();

    0
}

pub(crate) struct FiberPool<'a> {
    lua: &'a Lua,
    executor: Executor,
    config: ModuleConfig,

    scheduler: fiber::Fiber<'a, SchedulerArgs<'a>>,
}

impl<'a> FiberPool<'a> {
    pub fn new(lua: &'a Lua, executor: Executor, config: ModuleConfig) -> Self {
        let mut scheduler = fiber::Fiber::new("scheduler", &mut scheduler_f);
        scheduler.set_joinable(true);
        Self {
            lua,
            executor,
            config,
            scheduler,
        }
    }

    pub fn run(&mut self) -> std::io::Result<()> {
        self.scheduler.start(SchedulerArgs {
            lua: self.lua,
            executor: self.executor.try_clone()?,
            config: self.config.clone(),
        });
        Ok(())
    }

    // join will exit when all Dispatchers die
    pub fn join(&self) {
        self.scheduler.join();
    }
}
