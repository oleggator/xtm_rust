use tokio::sync::oneshot;
use async_channel;
use async_channel::TryRecvError;
use tarantool::fiber;
use tokio::runtime;
use std::future::Future;

type Task<'a> = Box<dyn FnOnce() -> Result<(), ExecError> + Send + 'a>;
type TaskTX<'a> = async_channel::Sender<Task<'a>>;

#[derive(Clone)]
pub struct Dispatcher<'a> {
    task_tx: TaskTX<'a>,
}

impl<'a> Dispatcher<'a> {
    pub fn new(task_tx: TaskTX) -> Dispatcher {
        Dispatcher { task_tx }
    }

    pub async fn call<F, R>(&self, func: F) -> Result<R, oneshot::error::RecvError>
        where
            R: Send + 'a,
            F: FnOnce() -> R + Send + 'a,
    {
        let (result_tx, result_rx) = oneshot::channel();
        let handler_func = Box::new(move || -> Result<(), ExecError> {
            let result: R = func();
            if let Err(_) = result_tx.send(result) {
                return Err(ExecError::TXChannelClosed);
            }
            Ok(())
        });

        self.task_tx.send(handler_func).await.unwrap();
        result_rx.await
    }
}

type TaskRX<'a> = async_channel::Receiver<Task<'a>>;

pub struct Executor<'a> {
    task_rx: TaskRX<'a>,
}

#[derive(Debug)]
pub enum ExecError {
    RXChannelClosed,
    TXChannelClosed,
}

impl<'a> Executor<'a> {
    pub fn new(task_rx: TaskRX) -> Executor {
        Executor { task_rx }
    }

    pub fn exec(&self) -> Result<(), ExecError> {
        let func = loop {
            match self.task_rx.try_recv() {
                Ok(result) => break result,
                Err(TryRecvError::Empty) => fiber::sleep(0.0),
                Err(TryRecvError::Closed) => return Err(ExecError::RXChannelClosed),
            };
        };
        func()
    }
}

pub fn run_module<Fut, M>(buffer: usize, module_main: M)
    where
        M: FnOnce(Dispatcher<'static>) -> Fut + Send + 'static,
        Fut: Future<Output=()>,
{
    let (dispatcher, executor) = tx_channel(buffer);

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

    let mut tx_fiber = tarantool::fiber::Fiber::new("module_tx", &mut tx_main);
    tx_fiber.set_joinable(true);
    tx_fiber.start(executor);

    tx_fiber.join();
    module_thread.join().unwrap();
}

pub fn tx_channel<'a>(buffer: usize) -> (Dispatcher<'a>, Executor<'a>)
{
    let (task_tx, task_rx) = async_channel::bounded(buffer);
    (Dispatcher::new(task_tx), Executor::new(task_rx))
}

pub fn tx_main(executor: Box<Executor>) -> i32 {
    loop {
        match executor.exec() {
            Ok(_) => {}
            Err(ExecError::RXChannelClosed) => {
                break 0;
            }
            Err(err) => {
                println!("{:#?}", err);
                break 1;
            }
        }
    }
}
