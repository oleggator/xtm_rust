use tokio::sync::oneshot;
use async_channel;
use async_channel::TryRecvError;
use thiserror::Error;
use crate::eventfd;
use std::{convert::TryFrom, os::unix::io::{AsRawFd, RawFd}};
use std::io;

type Task<T> = Box<dyn FnOnce(T) -> Result<(), ChannelError> + Send>;
type TaskSender<T> = async_channel::Sender<Task<T>>;
type TaskReceiver<T> = async_channel::Receiver<Task<T>>;

#[derive(Error, Debug)]
pub enum ChannelError {
    #[error("tx channel is closed")]
    TXChannelClosed,

    #[error("rx channel is closed")]
    RXChannelClosed,

    #[error("io error")]
    IOError(io::Error),
}

pub struct Dispatcher<T> {
    pub(crate) task_tx: TaskSender<T>,
    pub(crate) eventfd: eventfd::EventFd,
}

impl<T> Dispatcher<T> {
    pub fn new(task_tx: TaskSender<T>, eventfd: eventfd::EventFd) -> Self<T> {
        Self { task_tx, eventfd }
    }

    pub fn try_clone(&self) -> std::io::Result<Self> {
        Ok(Self {
            task_tx: self.task_tx.clone(),
            eventfd: self.eventfd.try_clone()?,
        })
    }
}

pub struct AsyncDispatcher<T> {
    task_tx: TaskSender<T>,
    eventfd: eventfd::AsyncEventFd,
}

impl<T> AsyncDispatcher<T> {
    pub async fn call<Func, Ret>(&self, func: Func) -> Result<Ret, ChannelError>
        where
            Ret: Send + 'static,
            Func: FnOnce(T) -> Ret,
            Func: Send + 'static,
    {
        let (result_tx, result_rx) = oneshot::channel();
        let handler_func: Task<T> = Box::new(move |lua| {
            let result = func(lua);
            result_tx.send(result).or(Err(ChannelError::TXChannelClosed))
        });

        if let Err(_channel_closed) = self.task_tx.send(handler_func).await {
            return Err(ChannelError::TXChannelClosed);
        }

        if let Err(err) = self.eventfd.write(1).await {
            return Err(ChannelError::IOError(err));
        }

        result_rx.await.or(Err(ChannelError::RXChannelClosed))
    }

    pub fn try_clone(&self) -> io::Result<Self> {
        Ok(AsyncDispatcher {
            task_tx: self.task_tx.clone(),
            eventfd: self.eventfd.try_clone()?,
        })
    }
}

impl<T> TryFrom<Dispatcher<T>> for AsyncDispatcher<T> {
    type Error = io::Error;

    fn try_from(dispatcher: Dispatcher<T>) -> Result<Self<T>, Self::Error> {
        Ok(Self {
            task_tx: dispatcher.task_tx,
            eventfd: eventfd::AsyncEventFd::try_from(dispatcher.eventfd)?,
        })
    }
}


pub struct Executor<T> {
    task_rx: TaskReceiver<T>,
    eventfd: eventfd::EventFd,
}

impl<T> Executor<T> {
    pub fn new(task_rx: TaskReceiver<T>, eventfd: eventfd::EventFd) -> Self<T> {
        Self { task_rx, eventfd }
    }

    pub fn exec(&self, lua: T, max_recv_retries: usize, coio_timeout: f64) -> Result<(), ChannelError> {
        loop {
            match self.task_rx.try_recv() {
                Ok(func) => return func(lua),
                Err(TryRecvError::Empty) => (),
                Err(TryRecvError::Closed) => return Err(ChannelError::RXChannelClosed),
            };

            for _ in 0..max_recv_retries {
                match self.task_rx.try_recv() {
                    Ok(func) => return func(lua),
                    Err(TryRecvError::Empty) => tarantool::fiber::sleep(0.),
                    Err(TryRecvError::Closed) => return Err(ChannelError::RXChannelClosed),
                };
            }
            let _ = self.eventfd.coio_read(coio_timeout);
        }
    }

    pub fn try_clone(&self) -> io::Result<Self> {
        Ok(Self {
            task_rx: self.task_rx.clone(),
            eventfd: self.eventfd.try_clone()?,
        })
    }
}

impl<T> AsRawFd for Executor<T> {
    fn as_raw_fd(&self) -> RawFd {
        self.eventfd.as_raw_fd()
    }
}

pub fn channel<T>(buffer: usize) -> io::Result<(Dispatcher<T>, Executor<T>)> {
    let (task_tx, task_rx) = async_channel::bounded(buffer);
    let efd = eventfd::EventFd::new(0, false)?;

    Ok((Dispatcher::new(task_tx, efd.try_clone()?), Executor::new(task_rx, efd)))
}
