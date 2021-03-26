use std::{rc::Rc, sync::Arc, os::unix::io::AsRawFd, sync::atomic::{AtomicI8, Ordering}, time::{SystemTime, UNIX_EPOCH}, io::{Read, Write}, future::Future, cell::RefCell, thread, time, io, fmt};
use std::collections::VecDeque;

use tarantool::{fiber::Fiber, ffi::tarantool::{CoIOFlags}, coio::coio_wait, fiber};

use tokio::{
    runtime,
    io::unix::AsyncFd,
    io::Interest,
};

use os_pipe::pipe;
use mlua::prelude::*;
use tarantool::fiber::fiber_yield;
use tarantool::ffi::tarantool::fiber_sleep;


enum State {
    Stopped = 0,
    Running = 1,
    Error = 2,
}

struct Module<'a> {
    is_running: Arc<AtomicI8>,

    module_thread: Option<thread::JoinHandle<()>>,
    tx_fiber: Option<tarantool::fiber::Fiber<'a, ()>>,
}

impl<'a> Module<'a> {
    pub fn new() -> Self {
        Module {
            is_running: Arc::new(AtomicI8::new(State::Stopped as i8)),

            module_thread: None,
            tx_fiber: None,
        }
    }

    pub fn cfg(&mut self) {
        let (module_tx, module_rx) = std::sync::mpsc::channel();
        let (module_waiter, mut module_notifier) = pipe().unwrap();

        let mut tx_fiber = Fiber::new("tx_fiber", &mut |_| {
            let mut module_tx = module_tx.clone();
            let mut module_notifier = module_notifier.try_clone().unwrap();

            println!("tx fiber started");

            let mut seq = 0;
            while is_running.load(Ordering::SeqCst) == State::Running as i8 {
                let ts: u64 = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
                    .into();

                module_tx.send(move || async move {
                    println!("ran in module thread: {}:{}", seq, ts);
                }).unwrap();

                let buffer = [0; 8];
                let fd = module_notifier.as_raw_fd();
                match coio_wait(fd, CoIOFlags::WRITE, 0.0) {
                    Ok(()) => {
                        module_notifier.write(&buffer).unwrap();
                    }
                    Err(_) => continue,
                };

                seq += 1;
                fiber::sleep(1.0);
            }
            0
        });
        tx_fiber.set_joinable(true);


        self.is_running.store(State::Running as i8, Ordering::SeqCst);
        tx_fiber.start(());
        let module_thread = thread::Builder::new()
            .name("module".to_string())
            .spawn(|| {
                runtime::Builder::new_current_thread()
                    .enable_io()
                    .build()
                    .unwrap()
                    .block_on(Self::module_main(module_rx, module_waiter));
            })
            .unwrap();

        self.module_thread = Some(module_thread);
        self.tx_fiber = Some(tx_fiber);
    }

    pub fn stop(&mut self) {
        if self.is_running.load(Ordering::SeqCst) == State::Running as i8 {
            self.is_running.store(State::Stopped as i8, Ordering::SeqCst);
            self.module_thread
                .take().unwrap()
                .join().unwrap();
            self.tx_fiber
                .take().unwrap()
                .join();
        }
    }

    async fn module_main<F, Fut>(rx: std::sync::mpsc::Receiver<F>, pipe_rx: os_pipe::PipeReader)
        where
            F: FnOnce() -> Fut,
            Fut: Future<Output=()>,
    {
        use std::borrow::Borrow;
        let notifier = AsyncFd::with_interest(pipe_rx, Interest::READABLE).unwrap();
        loop {
            let func: F = Self::recv(rx.borrow(), notifier.borrow()).await.unwrap();
            func().await;
        }
    }

    async fn recv<T>(rx: &std::sync::mpsc::Receiver<T>, notifier: &AsyncFd<os_pipe::PipeReader>) -> io::Result<T> {
        let mut buffer = [0; 8];
        loop {
            let mut guard = notifier.readable().await?;
            match guard.try_io(|inner| inner.get_ref().read(&mut buffer)) {
                Ok(size) => if size.unwrap() > 0 {
                    return Ok(rx.recv().unwrap());
                },
                Err(_would_block) => continue,
            }
        }
    }
}

struct Msg {
    seq: u64,
    ts: u64,
}

impl fmt::Display for Msg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}, {})", self.seq, self.ts)
    }
}

#[mlua::lua_module]
fn liblua(lua: &Lua) -> LuaResult<LuaTable> {
    let module = Rc::new(RefCell::new(Module::new()));

    let exports = lua.create_table()?;

    exports.set("cfg", {
        let module = module.clone();
        lua.create_function_mut(move |_: &Lua, _: ()| {
            module.borrow_mut().cfg();
            Ok(())
        })?
    })?;

    exports.set("stop", {
        let module = module.clone();
        lua.create_function_mut(move |_: &Lua, _: ()| {
            module.borrow_mut().stop();
            Ok(())
        })?
    })?;

    exports.set("with_yield", lua.create_function_mut(move |_: &Lua, _: ()| {
        let mut fiber = Fiber::new("test_fiber", &mut |_| {
            loop {
                println!("test");
                fiber::sleep(1.0);
            }
            0
        });
        fiber.set_joinable(true);
        fiber.start(());
        fiber.join();

        Ok("ok")
    })?)?;

    Ok(exports)
}
