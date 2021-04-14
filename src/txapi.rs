use tokio::sync::oneshot;
use async_channel;
use async_channel::TryRecvError;
use thiserror::Error;
use crate::eventfd;
use std::os::unix::io::{AsRawFd, RawFd};

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

pub struct Dispatcher<'a> {
    task_tx: TaskSender<'a>,
    eventfd: eventfd::EventFd,
}

impl<'a> Dispatcher<'a> {
    pub fn new(task_tx: TaskSender, eventfd: eventfd::EventFd) -> Dispatcher {
        Dispatcher { task_tx, eventfd }
    }

    pub fn try_clone(&self) -> std::io::Result<Self> {
        Ok(Dispatcher {
            task_tx: self.task_tx.clone(),
            eventfd: self.eventfd.try_clone()?,
        })
    }

    pub fn try_as_async_dispatcher(self) -> std::io::Result<AsyncDispatcher<'a>> {
        Ok(AsyncDispatcher {
            task_tx: self.task_tx,
            eventfd: eventfd::AsyncEventFd::try_from_eventfd(self.eventfd)?,
        })
    }
}

pub struct AsyncDispatcher<'a> {
    task_tx: TaskSender<'a>,
    eventfd: eventfd::AsyncEventFd,
}

impl<'a> AsyncDispatcher<'a> {
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
        self.eventfd.write(1).await.unwrap();

        result_rx.await.or(Err(ChannelError::RXChannelClosed))
    }

    pub fn try_clone(&self) -> std::io::Result<Self> {
        Ok(AsyncDispatcher {
            task_tx: self.task_tx.clone(),
            eventfd: self.eventfd.try_clone()?,
        })
    }
}


pub struct Executor<'a> {
    task_rx: TaskReceiver<'a>,
    eventfd: eventfd::EventFd,
}

impl<'a> Executor<'a> {
    pub fn new(task_rx: TaskReceiver, eventfd: eventfd::EventFd) -> Executor {
        Executor { task_rx, eventfd }
    }

    pub fn exec(&self) -> Result<(), ChannelError> {
        loop {
            for _ in 0..100 {
                match self.task_rx.try_recv() {
                    Ok(func) => return func(),
                    Err(TryRecvError::Empty) => tarantool::fiber::sleep(0.),
                    Err(TryRecvError::Closed) => return Err(ChannelError::RXChannelClosed),
                };
            }
            let _ = self.eventfd.coio_read(1.0);
        }
    }
}

impl<'a> AsRawFd for Executor<'a> {
    fn as_raw_fd(&self) -> RawFd {
        self.eventfd.as_raw_fd()
    }
}

pub fn channel<'a>(buffer: usize) -> (Dispatcher<'a>, Executor<'a>) {
    let (task_tx, task_rx) = async_channel::bounded(buffer);
    let efd = eventfd::EventFd::new(0, false).unwrap();

    (Dispatcher::new(task_tx, efd.try_clone().unwrap()), Executor::new(task_rx, efd))
}
