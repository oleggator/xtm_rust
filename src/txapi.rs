use tokio::sync::oneshot;
use async_channel;
use async_channel::TryRecvError;
use thiserror::Error;
use crate::eventfd;
use std::{convert::TryFrom, os::unix::io::{AsRawFd, RawFd}};
use std::io;
use mlua::Lua;
use tarantool::fiber;

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
    pub fn new(task_tx: TaskSender, eventfd: eventfd::EventFd) -> Dispatcher {
        Dispatcher { task_tx, eventfd }
    }

    pub fn try_clone(&self) -> std::io::Result<Self> {
        Ok(Dispatcher {
            task_tx: self.task_tx.clone(),
            eventfd: self.eventfd.try_clone()?,
        })
    }
}

pub struct AsyncDispatcher {
    task_tx: TaskSender,
    eventfd: eventfd::AsyncEventFd,
}

impl AsyncDispatcher {
    pub async fn call<Func, Ret>(&self, func: Func) -> Result<Ret, ChannelError>
        where
            Ret: Send + 'static,
            Func: Fn(&Lua) -> Ret,
            Func: Send + 'static,
    {
        let (result_tx, result_rx) = oneshot::channel();
        let handler_func: Task = Box::new(move |lua| {

            let mut fib = fiber::Fiber::new("fiber", &mut |args: Box<(oneshot::Sender<Ret>, Func)>| {
                let (result_tx, lua_func) = (args.0, args.1);

                let result = lua_func(lua);
                if let Err(_) = result_tx.send(result) {
                    println!("channel has been closed");
                    return -1;
                }

                return 0;
            });
            fib.set_joinable(true);

            fib.start((result_tx, func));
            // if fib.join() != 0 {
            //     return Err(ChannelError::TXChannelClosed);
            // }

            Ok(())
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

impl TryFrom<Dispatcher> for AsyncDispatcher {
    type Error = io::Error;

    fn try_from(dispatcher: Dispatcher) -> Result<AsyncDispatcher, Self::Error> {
        Ok(AsyncDispatcher {
            task_tx: dispatcher.task_tx,
            eventfd: eventfd::AsyncEventFd::try_from(dispatcher.eventfd)?,
        })
    }
}


pub struct Executor {
    task_rx: TaskReceiver,
    eventfd: eventfd::EventFd,
}

impl Executor {
    pub fn new(task_rx: TaskReceiver, eventfd: eventfd::EventFd) -> Executor {
        Executor { task_rx, eventfd }
    }

    pub fn exec(&self, lua: &Lua) -> Result<(), ChannelError> {
        loop {
            loop {
                match self.task_rx.try_recv() {
                    Ok(func) => return func(lua),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Closed) => return Err(ChannelError::RXChannelClosed),
                };
            }
            let _ = self.eventfd.coio_read(1.0);
        }
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
