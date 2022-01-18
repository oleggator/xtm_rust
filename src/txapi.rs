use tokio::sync::oneshot;
use async_channel;
use thiserror::Error;
use crate::eventfd;
use std::os::unix::io::{AsRawFd, RawFd};
use std::io;
use std::sync::Arc;
use mlua::Lua;
use tokio::pin;
use futures::Future;
use futures::task::waker_ref;
use std::task::{Context, Poll};

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

#[derive(Clone)]
pub struct Dispatcher {
    pub(crate) task_tx: TaskSender,
}

impl Dispatcher {
    pub fn new(task_tx: TaskSender) -> Self {
        Self { task_tx }
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
        
        if let Err(_channel_closed) = self.task_tx.send(handler_func).await {
            return Err(ChannelError::TXChannelClosed);
        }

        result_rx.await.or(Err(ChannelError::RXChannelClosed))
    }

    pub fn len(&self) -> usize {
        self.task_tx.len()
    }
}


#[derive(Clone)]
pub struct Executor {
    task_rx: TaskReceiver,
    eventfd: Arc<eventfd::EventFd>,
}

impl Executor {
    pub fn new(task_rx: TaskReceiver, eventfd: eventfd::EventFd) -> Self {
        Self { task_rx, eventfd: Arc::new(eventfd) }
    }

    pub fn exec(&self, lua: &Lua, coio_timeout: f64) -> Result<(), ChannelError> {
        let f = self.task_rx.recv();
        pin!(f);

        let waker = waker_ref(&self.eventfd);
        let cx = &mut Context::from_waker(&waker);

        loop {
            match f.as_mut().poll(cx) {
                Poll::Ready(result) => return match result {
                    Ok(func) => func(lua),
                    Err(_) => Err(ChannelError::RXChannelClosed),
                },
                Poll::Pending => { let _ = self.eventfd.coio_read(coio_timeout); },
            };
        };
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

    Ok((Dispatcher::new(task_tx), Executor::new(task_rx, efd)))
}
