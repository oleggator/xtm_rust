use crate::txapi::{channel, AsyncDispatcher};
use std::io;
use std::{convert::TryFrom, future::Future};
use tokio::runtime;

use crossbeam_utils::thread;

mod eventfd;
pub mod txapi;

pub fn run_module<Fut, M, T>(buffer: usize, module_main: M) -> io::Result<T>
where
    M: FnOnce(AsyncDispatcher) -> Fut,
    M: Send,
    Fut: Future<Output = T>,
    T: Send,
{
    let (dispatcher, executor) = channel(buffer)?;

    let result = thread::scope(|scope| {
        let module_thread = scope
            .builder()
            .name("module".to_string())
            .spawn(move |_| -> io::Result<T> {
                let rt = runtime::Builder::new_multi_thread().enable_io().build()?;
                rt.block_on(async move {
                    let async_dispather = AsyncDispatcher::try_from(dispatcher)?;
                    Ok(module_main(async_dispather).await)
                })
            })
            .unwrap();

        while executor.exec().is_ok() {}
        module_thread.join().unwrap().unwrap()
    })
    .unwrap();

    Ok(result)
}
