use mlua::prelude::*;
use tokio::sync::oneshot;
use async_channel;
use tokio::sync::oneshot::error::RecvError;
use async_channel::TryRecvError;
use tokio::runtime;

type Task<F, R> = (F, oneshot::Sender<R>);
type TaskTX<F, R> = async_channel::Sender<Task<F, R>>;

pub struct Dispatcher<F, R> {
    task_tx: TaskTX<F, R>,
}

impl<F, R> Dispatcher<F, R> where F: FnOnce() -> R + Send + 'static {
    pub fn new(task_tx: TaskTX<F, R>) -> Dispatcher<F, R> {
        Dispatcher { task_tx }
    }

    pub async fn call(&self, func: F) -> Result<R, RecvError> {
        let (result_tx, result_rx) = oneshot::channel();
        self.task_tx.send((func, result_tx)).await.unwrap();
        result_rx.await
    }
}

type TaskRX<F, R> = async_channel::Receiver<(F, oneshot::Sender<R>)>;

pub struct Executor<F, R> {
    task_rx: TaskRX<F, R>,
}

#[derive(Debug)]
pub enum ExecError {
    RXChannelClosed,
    TXChannelClosed,
}

impl<F, R> Executor<F, R> where F: FnOnce() -> R + Send + 'static {
    pub fn new(task_rx: TaskRX<F, R>) -> Executor<F, R> {
        Executor { task_rx }
    }

    pub fn exec(&self) -> Result<(), ExecError> {
        let (func, result_tx) = loop {
            match self.task_rx.try_recv() {
                Ok(result) => break result,
                Err(TryRecvError::Empty) => {
                    // coio_wait
                    tarantool::fiber::fiber_yield();
                    continue;
                }
                Err(TryRecvError::Closed) => return Err(ExecError::RXChannelClosed),
            };
        };

        let result: R = func();
        if let Err(_) = result_tx.send(result) {
            return Err(ExecError::TXChannelClosed);
        }
        Ok(())
    }
}

pub fn tx_channel<F, R>(buffer: usize) -> (Dispatcher<F, R>, Executor<F, R>)
    where F: FnOnce() -> R + Send + 'static
{
    let (task_tx, task_rx) = async_channel::bounded(buffer);
    (Dispatcher::new(task_tx), Executor::new(task_rx))
}

async fn module_main(dispatcher: Dispatcher<Box<dyn FnOnce() -> i32 + Send + 'static>, i32>) {
    dispatcher.call(Box::new(move || {
        println!("it works!");
        0
    })).await.unwrap();
}

#[mlua::lua_module]
fn libtxapi(lua: &Lua) -> LuaResult<LuaTable> {
    let exports = lua.create_table()?;

    type R = i32;
    type F = Box<dyn FnOnce() -> R + Send + 'static>;

    exports.set("start", lua.create_function_mut(|_: &Lua, _: ()| {
        let (
            dispatcher,
            executor,
        ) = tx_channel::<F, R>(128);

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

        let mut tx_fiber = tarantool::fiber::Fiber::new("module_tx", &mut |executor: Box<Executor<F, R>>| {
            loop {
                match executor.exec() {
                    Ok(_) => {}
                    Err(err) => {
                        println!("{:#?}", err);
                        break;
                    }
                }
            }
            0
        });
        tx_fiber.set_joinable(true);
        tx_fiber.start(executor);

        module_thread.join().unwrap();
        tx_fiber.join();
        Ok(())
    })?)?;

    Ok(exports)
}
