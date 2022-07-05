use crate::eventfd;
use async_channel;
use async_channel::TryRecvError;
use mlua::Lua;
use tracing_opentelemetry::OpenTelemetrySpanExt;
use std::io;
use std::os::unix::io::{AsRawFd, RawFd};
use thiserror::Error;
use tokio::sync::oneshot;
use tracing::Instrument;
use opentelemetry::Context;

pub type Task = Box<dyn FnOnce(&Lua, Context) -> Result<(), ChannelError> + Send>;
pub type InstrumentedTask = (Task, Context);
type TaskSender = async_channel::Sender<InstrumentedTask>;
type TaskReceiver = async_channel::Receiver<InstrumentedTask>;

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

    #[tracing::instrument(level = "trace", skip_all)]
    pub async fn call<Func, Ret>(&self, func: Func) -> Result<Ret, ChannelError>
    where
        Ret: Send + 'static,
        Func: FnOnce(&Lua, Context) -> Ret,
        Func: Send + 'static,
    {
        tracing::event!(tracing::Level::TRACE, "bass drop begin");

        let (result_tx, result_rx) = oneshot::channel();
        let result_rx = result_rx
            .instrument(tracing::span!(tracing::Level::TRACE, "result_rx"));
        let result_rx_span_ctx = result_rx.span().context();

        let handler_func: Task = Box::new(move |lua, exec_ctx| {
            if result_tx.is_closed() {
                return Err(ChannelError::TXChannelClosed);
            };

            let result = func(lua, exec_ctx);
            result_tx
                .send(result)
                .or(Err(ChannelError::TXChannelClosed))
        });

        let task_tx_len = self.task_tx.len();
        if let Err(_channel_closed) = self.task_tx.send((handler_func, result_rx_span_ctx)).await {
            return Err(ChannelError::TXChannelClosed);
        }

        if task_tx_len == 0 {
            if let Err(err) = self.eventfd.write(1) {
                return Err(ChannelError::IOError(err));
            }
        }

        tracing::event!(tracing::Level::TRACE, "bass drop end");

        result_rx
            .await
            .or(Err(ChannelError::RXChannelClosed))
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

    // #[tracing::instrument(level = "trace", skip_all)]
    pub fn exec(&self, lua: &Lua, coio_timeout: f64) -> Result<(), ChannelError> {
        loop {
            // tracing::event!(tracing::Level::TRACE, "exec: iteration");

            // let _ = tracing::trace_span!("executing task").enter();
            match self.task_rx.try_recv()
             {
                Ok((func, span_ctx)) => {
                    // tracing::event!(tracing::Level::TRACE, "task: start");
                    println!("{:?}", span_ctx);

                    let res = func(lua, span_ctx);

                    // tracing::event!(tracing::Level::TRACE, "task: finish");
                    return res;
                }
                Err(TryRecvError::Empty) => (),
                Err(TryRecvError::Closed) => return Err(ChannelError::RXChannelClosed),
            };

            let _ = self.eventfd.coio_read(coio_timeout);
        }
    }

    // #[tracing::instrument(level = "trace", skip_all)]
    pub fn pop_many(&self, max_tasks: usize, coio_timeout: f64) -> Result<Vec<InstrumentedTask>, ChannelError> {
        if self.task_rx.is_empty() {
            let _ = self.eventfd.coio_read(coio_timeout);
        }

        let mut tasks = Vec::with_capacity(max_tasks);
        for _ in 0..max_tasks {
            match self.task_rx.try_recv() {
                Ok(func) => tasks.push(func),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Closed) => return Err(ChannelError::RXChannelClosed),
            };

            if self.task_rx.len() <= 1 {
                break;
            }
        }

        Ok(tasks)
    }

    pub fn try_clone(&self) -> io::Result<Self> {
        Ok(Self {
            task_rx: self.task_rx.clone(),
            eventfd: self.eventfd.try_clone()?,
        })
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

    Ok((
        Dispatcher::new(task_tx, efd.try_clone()?),
        Executor::new(task_rx, efd),
    ))
}
