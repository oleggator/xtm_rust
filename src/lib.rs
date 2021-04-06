use crate::txapi::{Dispatcher, channel};
use std::future::Future;
use tokio::runtime;

pub mod txapi;

pub fn run_module<Fut, M>(buffer: usize, module_main: M)
    where
        M: FnOnce(Dispatcher<'static>) -> Fut + Send + 'static,
        Fut: Future<Output=()>,
{
    let (dispatcher, executor) = channel(buffer);

    let module_thread = std::thread::Builder::new()
        .name("module".to_string())
        .spawn(move || {
            runtime::Builder::new_multi_thread()
                .enable_io()
                .build()
                .unwrap()
                .block_on(module_main(dispatcher))
        })
        .unwrap();

    while executor.exec().is_ok() {}
    module_thread.join().unwrap();
}
