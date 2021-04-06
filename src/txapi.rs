use tokio::sync::oneshot;
use async_channel;
use async_channel::TryRecvError;
use tarantool::fiber;
use thiserror::Error;

type Task<'a> = Box<dyn FnOnce() -> Result<(), ChannelError> + Send + 'a>;
type TaskSender<'a> = async_channel::Sender<Task<'a>>;
type TaskReceiver<'a> = async_channel::Receiver<Task<'a>>;

#[derive(Error, Debug)]
pub enum ChannelError {
    #[error("tx channel is closed")]
    TXChannelClosed,

    #[error("rx channel is closed")]
    RXChannelClosed,
}

#[derive(Clone)]
pub struct Dispatcher<'a> {
    task_tx: TaskSender<'a>,
}

impl<'a> Dispatcher<'a> {
    pub fn new(task_tx: TaskSender) -> Dispatcher {
        Dispatcher { task_tx }
    }

    pub async fn call<F, R>(&self, func: F) -> Result<R, ChannelError>
        where
            R: Send + 'a,
            F: FnOnce() -> R + Send + 'a,
    {
        let (result_tx, result_rx) = oneshot::channel();
        let handler_func = Box::new(move || {
            let result: R = func();
            result_tx.send(result).or(Err(ChannelError::TXChannelClosed))
        });

        if let Err(_channel_closed) = self.task_tx.send(handler_func).await {
            return Err(ChannelError::TXChannelClosed);
        }

        result_rx.await.or(Err(ChannelError::RXChannelClosed))
    }
}

pub struct Executor<'a> {
    task_rx: TaskReceiver<'a>,
}

impl<'a> Executor<'a> {
    pub fn new(task_rx: TaskReceiver) -> Executor {
        Executor { task_rx }
    }

    pub fn exec(&self) -> Result<(), ChannelError> {
        loop {
            match self.task_rx.try_recv() {
                Ok(func) => break func(),
                Err(TryRecvError::Empty) => fiber::sleep(0.0),
                Err(TryRecvError::Closed) => break Err(ChannelError::RXChannelClosed),
            };
        }
    }
}

pub fn channel<'a>(buffer: usize) -> (Dispatcher<'a>, Executor<'a>) {
    let (task_tx, task_rx) = async_channel::bounded(buffer);
    (Dispatcher::new(task_tx), Executor::new(task_rx))
}
