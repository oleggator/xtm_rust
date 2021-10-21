use tokio::sync::oneshot;
use async_channel;
use async_channel::TryRecvError;
use thiserror::Error;
use std::{convert::TryFrom, os::unix::io::{AsRawFd, RawFd}};
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

}

impl Dispatcher {
    pub fn new(task_tx: TaskSender) -> Self {
        Self { task_tx }
    }

    pub fn try_clone(&self) -> std::io::Result<Self> {
        Ok(Self {
            task_tx: self.task_tx.clone(),
        })
    }
}

pub struct AsyncDispatcher {
    task_tx: TaskSender,
}

impl AsyncDispatcher {
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

        if let Err(_channel_closed) = self.task_tx.send(handler_func).await {
            return Err(ChannelError::TXChannelClosed);
        }

        result_rx.await.or(Err(ChannelError::RXChannelClosed))
    }

    pub fn try_clone(&self) -> io::Result<Self> {
        Ok(AsyncDispatcher {
            task_tx: self.task_tx.clone(),
        })
    }
}

impl TryFrom<Dispatcher> for AsyncDispatcher {
    type Error = io::Error;

    fn try_from(dispatcher: Dispatcher) -> Result<Self, Self::Error> {
        Ok(Self {
            task_tx: dispatcher.task_tx,
        })
    }
}


pub struct Executor {
    task_rx: TaskReceiver,
}

impl Executor {
    pub fn new(task_rx: TaskReceiver) -> Self {
        Self { task_rx }
    }

    pub fn exec(&self, lua: &Lua) -> Result<(), ChannelError> {
        loop {
            match self.task_rx.try_recv() {
                Ok(func) => return func(lua),
                Err(TryRecvError::Empty) => tarantool::fiber::sleep(0.),
                Err(TryRecvError::Closed) => return Err(ChannelError::RXChannelClosed),
            };
        }
    }

    pub fn try_clone(&self) -> io::Result<Self> {
        Ok(Self {
            task_rx: self.task_rx.clone(),
        })
    }
}

pub fn channel(buffer: usize) -> io::Result<(Dispatcher, Executor)> {
    let (task_tx, task_rx) = async_channel::bounded(buffer);

    Ok((Dispatcher::new(task_tx), Executor::new(task_rx)))
}
