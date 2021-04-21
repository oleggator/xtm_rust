use crate::txapi::{AsyncDispatcher, channel};
use std::{convert::TryFrom, future::Future};
use tokio::runtime;
use std::io;

pub mod txapi;
mod eventfd;

pub fn run_module<Fut, M>(buffer: usize, module_main: M) -> io::Result<()>
    where
        M: FnOnce(AsyncDispatcher<'static>) -> Fut + Send + 'static,
        Fut: Future<Output=()> + Send + 'static,
{
    let (dispatcher, executor) = channel(buffer)?;

    let module_thread = std::thread::Builder::new()
        .name("module".to_string())
        .spawn(move || -> io::Result<()> {
            runtime::Builder::new_multi_thread()
                .enable_io()
                .build()
                .unwrap()
                .block_on(async move {
                    let async_dispather = AsyncDispatcher::try_from(dispatcher)?;
                    module_main(async_dispather).await;
                    Ok(())
                })
        })
        .unwrap();

    while executor.exec().is_ok() {}
    module_thread.join().unwrap()
}
