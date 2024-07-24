use crate::notify::Notify;
use parking_lot::Mutex;
use std::cell::RefCell;
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::oneshot;

type Task<T> = Box<dyn FnOnce(&T) -> Result<(), ExecError> + Send>;

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

struct Channel<T> {
    buf: Mutex<Vec<Task<T>>>,
    rx_notify: tokio::sync::Notify,

    dispatchers: AtomicUsize,
}

pub struct Dispatcher<T> {
    channel: Arc<Channel<T>>,
    tx_notify: Notify,
}

impl<T> Dispatcher<T> {
    pub fn try_clone(&self) -> io::Result<Self> {
        self.channel.dispatchers.fetch_add(1, Ordering::Relaxed);
        Ok(Self {
            channel: self.channel.clone(),
            tx_notify: self.tx_notify.try_clone()?,
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

        loop {
            {
                let mut write_buf = self.channel.buf.lock();
                if write_buf.len() < write_buf.capacity() {
                    write_buf.push(handler_func);
                    break;
                }
            }

            self.channel.rx_notify.notified().await;
        }

        self.tx_notify.notify(1).unwrap();
        result_rx.await.or(Err(CallError::ResultChannelRecvError))
    }
}

impl<T> Drop for Dispatcher<T> {
    fn drop(&mut self) {
        self.channel.dispatchers.fetch_sub(1, Ordering::Relaxed);
        self.tx_notify.notify(1).unwrap();
    }
}

pub struct Executor<T> {
    chan: Arc<Channel<T>>,
    read_buf: RefCell<Vec<Task<T>>>,
    tx_notify: Notify,
    // begin: Cell<usize>,
}

impl<T> Executor<T> {
    pub fn exec(&self, arg: &T, coio_timeout: f64) -> Result<(), ExecError> {
        if let Some(func) = self.read_buf.borrow_mut().pop() {
            return func(arg);
        }

        loop {
            let dispatchers = self.chan.dispatchers.load(Ordering::Relaxed);
            if dispatchers == 0 {
                return Err(ExecError::TaskChannelRecvError);
            }

            {
                let mut write_buf = self.chan.buf.lock();
                std::mem::swap(&mut *self.read_buf.borrow_mut(), &mut *write_buf);
                self.chan.rx_notify.notify_one();
            }

            // TODO: we should pop from begin
            if let Some(func) = self.read_buf.borrow_mut().pop() {
                return func(arg);
            }

            if let Ok(n) = self.tx_notify.notified_coio(coio_timeout) {
                // println!("{}", n);
            } else {
                eprint!("*");
            }

            // let _ = &self.tx_notify;
            // tarantool::fiber::sleep(std::time::Duration::ZERO);
        }
    }
}

pub fn channel<T>(buffer: usize) -> io::Result<(Dispatcher<T>, Executor<T>)> {
    let tx_notify = Notify::new(0, false)?;
    let rx_notify = tokio::sync::Notify::new();

    let buf = Mutex::new(Vec::with_capacity(buffer));
    let chan = Arc::new(Channel {
        buf,
        rx_notify,
        dispatchers: AtomicUsize::new(1),
    });

    Ok((
        Dispatcher {
            channel: chan.clone(),
            tx_notify: tx_notify.try_clone()?,
        },
        Executor {
            chan,
            read_buf: RefCell::new(Vec::with_capacity(buffer)),
            tx_notify,
            // begin: 0,
        },
    ))
}
