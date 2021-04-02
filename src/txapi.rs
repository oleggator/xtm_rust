use tokio::sync::oneshot;
use async_channel;
use async_channel::TryRecvError;
use tarantool::fiber;

type Task<F, R> = (F, oneshot::Sender<R>);
type TaskTX<F, R> = async_channel::Sender<Task<F, R>>;

pub struct Dispatcher<F, R> {
    task_tx: TaskTX<F, R>,
}

impl<F, R> Dispatcher<F, R> where F: FnOnce() -> R + Send + 'static {
    pub fn new(task_tx: TaskTX<F, R>) -> Dispatcher<F, R> {
        Dispatcher { task_tx }
    }

    pub async fn call(&self, func: F) -> Result<R, oneshot::error::RecvError> {
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
                Ok(result) => {
                    break result;
                }
                Err(TryRecvError::Empty) => {
                    // TODO replace fiber sleep with coio_wait
                    fiber::sleep(0.0);
                }
                Err(TryRecvError::Closed) => {
                    return Err(ExecError::RXChannelClosed);
                }
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
