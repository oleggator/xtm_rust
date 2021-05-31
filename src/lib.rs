use crate::txapi::{AsyncDispatcher, channel};
use std::{convert::TryFrom, future::Future};
use tokio::runtime;
use std::io;

use crossbeam_utils::thread;

pub mod txapi;
mod eventfd;

pub fn run_module<Fut, M>(buffer: usize, module_main: M) -> io::Result<()>
    where
        M: FnOnce(AsyncDispatcher<'static>) -> Fut + Send + 'static,
        Fut: Future<Output=()> + Send + 'static,
{
    let (dispatcher, executor) = channel(buffer)?;

    thread::scope(|scope| {
        let module_thread = scope.builder()
            .name("module".to_string())
            .spawn(move |_| -> io::Result<()> {
                runtime::Builder::new_multi_thread()
                    .enable_io()
                    .build()?
                    .block_on(async move {
                        let async_dispather = AsyncDispatcher::try_from(dispatcher)?;
                        module_main(async_dispather).await;
                        Ok(())
                    })
            }).unwrap();

        while executor.exec().is_ok() {}
        module_thread.join().unwrap().unwrap();
    }).unwrap();
    Ok(())   
}
