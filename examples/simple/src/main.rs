use std::{thread, time, io};
use tokio::runtime;
use std::time::{SystemTime, UNIX_EPOCH};
use os_pipe::pipe;
use tokio::io::unix::AsyncFd;
use tokio::io::Interest;
use std::io::{Read, Write};
use std::borrow::Borrow;
use std::future::Future;


async fn module_main<F, Fut>(rx: std::sync::mpsc::Receiver<F>, pipe_rx: os_pipe::PipeReader)
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output=()>,
{
    let notifier = AsyncFd::with_interest(pipe_rx, Interest::READABLE).unwrap();
    loop {
        let func: F = recv(rx.borrow(), notifier.borrow()).await.unwrap();
        func().await;
    }
}

async fn recv<T>(rx: &std::sync::mpsc::Receiver<T>, notifier: &AsyncFd<os_pipe::PipeReader>) -> io::Result<T> {
    let mut buffer = [0; 64];
    loop {
        let mut guard = notifier.readable().await?;
        match guard.try_io(|inner| inner.get_ref().read(&mut buffer)) {
            Ok(size) => if size.unwrap() > 0 {
                return Ok(rx.recv().unwrap());
            },
            Err(_would_block) => continue,
        }
    }
}

fn main() {
    let (module_tx, module_rx) = std::sync::mpsc::channel();
    let (module_waiter, mut module_notifier) = pipe().unwrap();

    thread::Builder::new()
        .name("module".to_string())
        .spawn(|| {
            runtime::Builder::new_current_thread()
                .enable_io()
                .build()
                .unwrap()
                .block_on(module_main(module_rx, module_waiter));
        })
        .unwrap();

    let mut seq = 0;
    loop {
        let ts: u64 = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .into();

        module_tx.send(move || async move {
            println!("ran in module thread: {}:{}", seq, ts);
        }).unwrap();

        let buffer = [0; 64];
        module_notifier.write(&buffer).unwrap();

        seq += 1;
        thread::sleep(time::Duration::from_secs(1));
    }
}
