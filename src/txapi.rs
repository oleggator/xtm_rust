use tokio::sync::oneshot;
use async_channel;
use async_channel::TryRecvError;
use thiserror::Error;
use crate::eventfd;
use std::os::unix::io::{AsRawFd, RawFd};
use std::io;
use mlua::Lua;

type Task = Box<dyn FnOnce(&Lua) -> Result<(), ChannelError> + Send>;
type TaskSender = async_channel::Sender<Task>;
type TaskReceiver = async_channel::Receiver<Task>;

#[derive(Error, Debug)]
pub enum ChannelError {
    #[error("tx channel is closed")]
    TXChannelClosed,

    #[error("rx channel is closed")]
    RXChannelClosed,

    #[error("io error")]
    IOError(io::Error),
}

pub struct Dispatcher {
    pub(crate) task_tx: TaskSender,
    pub(crate) eventfd: eventfd::EventFd,
}

impl Dispatcher {
    pub fn new(task_tx: TaskSender, eventfd: eventfd::EventFd) -> Self {
        Self { task_tx, eventfd }
    }

    pub fn try_clone(&self) -> std::io::Result<Self> {
        Ok(Self {
            task_tx: self.task_tx.clone(),
            eventfd: self.eventfd.try_clone()?,
        })
    }

    pub async fn call<Func, Ret>(&self, func: Func) -> Result<Ret, ChannelError>
        where
            Ret: Send + 'static,
            Func: FnOnce(&Lua) -> Ret,
            Func: Send + 'static,
    {
        let (result_tx, result_rx) = oneshot::channel();
        let handler_func: Task = Box::new(move |lua| {
            if result_tx.is_closed() {
                return Err(ChannelError::TXChannelClosed)
            };

            let result = func(lua);
            result_tx.send(result).or(Err(ChannelError::TXChannelClosed))
        });
        
        let task_tx_len = self.task_tx.len();
        if let Err(_channel_closed) = self.task_tx.send(handler_func).await {
            return Err(ChannelError::TXChannelClosed);
        }

        if task_tx_len == 0 {
            if let Err(err) = self.eventfd.write(1) {
                return Err(ChannelError::IOError(err));
            }
        }

        result_rx.await.or(Err(ChannelError::RXChannelClosed))
    }

    pub fn len(&self) -> usize {
        self.task_tx.len()
    }
}


pub struct Executor {
    task_rx: TaskReceiver,
    eventfd: eventfd::EventFd,
}

impl Executor {
    pub fn new(task_rx: TaskReceiver, eventfd: eventfd::EventFd) -> Self {
        Self { task_rx, eventfd }
    }

    pub fn exec(&self, lua: &Lua, max_recv_retries: usize, coio_timeout: f64) -> Result<(), ChannelError> {
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

    pub fn len(&self) -> usize {
        self.task_rx.len()
    }
}

impl AsRawFd for Executor {
    fn as_raw_fd(&self) -> RawFd {
        self.eventfd.as_raw_fd()
    }
}

pub fn channel(buffer: usize) -> io::Result<(Dispatcher, Executor)> {
    let (task_tx, task_rx) = async_channel::bounded(buffer);
    let efd = eventfd::EventFd::new(0, false)?;

    Ok((Dispatcher::new(task_tx, efd.try_clone()?), Executor::new(task_rx, efd)))
}
