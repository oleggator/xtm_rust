use futures::future::join_all;
use mlua::prelude::*;
use tokio::time::Instant;
use xtm_rust::run_module;
use xtm_rust::txapi::AsyncDispatcher;
use hdrhistogram::{sync::SyncHistogram, Histogram};

async fn module_main(dispatcher: AsyncDispatcher<'static>) {
    let iterations = 10_000_000;

    let worker_n = 6;
    let iterations_per_worker = iterations / worker_n;
    let mut workers = Vec::new();

    let mut histogram: SyncHistogram<_> = Histogram::<u64>::new(5).unwrap().into();

    let begin = Instant::now();
    for i in 0..worker_n {
        let dispatcher = dispatcher.try_clone().unwrap();
        let mut recorder = histogram.recorder();

        let worker = tokio::spawn(async move {
            for j in i * iterations_per_worker..(i + 1) * iterations_per_worker {
                let begin = Instant::now();
                assert_eq!(dispatcher.call(move || j).await.unwrap(), j);
                recorder.record(begin.elapsed().as_nanos() as u64).unwrap();
            }
        });
        workers.push(worker);
    }
    join_all(workers).await;

    let elapsed = begin.elapsed();
    println!(
        "iterations: {}\nelapsed: {}ns\navg rps: {}",
        iterations,
        elapsed.as_nanos(),
        (iterations as f64 / elapsed.as_secs_f64()) as u64
    );

    histogram.refresh();
    println!(
        "p50: {}ns\np90: {}ns\np99: {}ns\nmean: {}ns\nmin: {}ns\nmax: {}ns",
        histogram.value_at_quantile(0.50),
        histogram.value_at_quantile(0.90),
        histogram.value_at_quantile(0.99),
        histogram.mean() as u64,
        histogram.min(),
        histogram.max(),
    );
}

#[mlua::lua_module]
fn libbench(lua: &Lua) -> LuaResult<LuaTable> {
    let exports = lua.create_table()?;

    exports.set(
        "start",
        lua.create_function_mut(|_, (buffer,)| {
            run_module(buffer, module_main).unwrap();
            Ok(())
        })?,
    )?;

    Ok(exports)
}