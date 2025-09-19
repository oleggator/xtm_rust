use crate::notify;
use async_channel::TryRecvError;
use notify::Notify;
use std::io;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::oneshot;

type Task<T> = Box<dyn FnOnce(&T) -> Result<(), ExecError> + Send>;
type TaskSender<T> = async_channel::Sender<Task<T>>;
type TaskReceiver<T> = async_channel::Receiver<Task<T>>;

#[derive(Error, Debug)]
pub enum CallError {
    #[error("task channel is closed")]
    TaskChannelSendError,

    #[error("result channel is closed")]
    ResultChannelRecvError,

    #[error("notify error")]
    NotifyError(io::Error),
}

#[derive(Error, Debug)]
pub enum ExecError {
    #[error("result channel is closed")]
    ResultChannelSendError,

    #[error("task channel is closed")]
    TaskChannelRecvError,
}

pub struct Dispatcher<T> {
    task_tx: TaskSender<T>,
    notify: Notify,
}

impl<T> Dispatcher<T> {
    pub fn try_clone(&self) -> io::Result<Self> {
        Ok(Self {
            task_tx: self.task_tx.clone(),
            notify: self.notify.try_clone()?,
        })
    }

    pub async fn call<Func, Ret>(&self, func: Func) -> Result<Ret, CallError>
    where
        Ret: Send + 'static,
        Func: FnOnce(&T) -> Ret,
        Func: Send + 'static,
    {
        let (result_tx, result_rx) = oneshot::channel();
        let handler_func: Task<T> = Box::new(move |arg| {
            if result_tx.is_closed() {
                return Err(ExecError::ResultChannelSendError);
            };

            let result = func(arg);
            result_tx
                .send(result)
                .or(Err(ExecError::ResultChannelSendError))
        });

        let task_tx_len = self.task_tx.len();
        if let Err(_channel_closed) = self.task_tx.send(handler_func).await {
            return Err(CallError::TaskChannelSendError);
        }

        if task_tx_len == 0
            && let Err(err) = self.notify.notify(1)
        {
            return Err(CallError::NotifyError(err));
        }

        result_rx.await.or(Err(CallError::ResultChannelRecvError))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.task_tx.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.task_tx.is_empty()
    }
}

pub struct Executor<T> {
    task_rx: TaskReceiver<T>,
    notify: Notify,
}

impl<T> Executor<T> {
    pub fn exec(
        &self,
        arg: &T,
        max_recv_retries: usize,
        coio_timeout: f64,
    ) -> Result<(), ExecError> {
        loop {
            match self.task_rx.try_recv() {
                Ok(func) => return func(arg),
                Err(TryRecvError::Empty) => (),
                Err(TryRecvError::Closed) => return Err(ExecError::TaskChannelRecvError),
            };

            for _ in 0..max_recv_retries {
                match self.task_rx.try_recv() {
                    Ok(func) => return func(arg),
                    Err(TryRecvError::Empty) => tarantool::fiber::sleep(Duration::new(0, 0)),
                    Err(TryRecvError::Closed) => return Err(ExecError::TaskChannelRecvError),
                };
            }
            let _ = self.notify.notified_coio(coio_timeout);
        }
    }

    pub fn try_clone(&self) -> io::Result<Self> {
        Ok(Self {
            task_rx: self.task_rx.clone(),
            notify: self.notify.try_clone()?,
        })
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.task_rx.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.task_rx.is_empty()
    }
}

pub fn channel<T>(buffer: usize) -> io::Result<(Dispatcher<T>, Executor<T>)> {
    let (task_tx, task_rx) = async_channel::bounded(buffer);
    let notify = Notify::new(0, false)?;

    Ok((
        Dispatcher {
            task_tx,
            notify: notify.try_clone()?,
        },
        Executor { task_rx, notify },
    ))
}
