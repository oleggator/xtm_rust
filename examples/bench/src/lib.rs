use futures::future::join_all;
use hdrhistogram::{Histogram, sync::SyncHistogram};
use mlua::prelude::*;
use tokio::time::Instant;
use xtm_rust::{Dispatcher, ModuleConfig, run_module_with_mlua};

async fn module_main(dispatcher: Dispatcher<Lua>) {
    let iterations = 10_000_000;

    let worker_n = 128;
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
                assert_eq!(dispatcher.call(move |_| j).await.unwrap(), j);
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
fn bench(lua: &Lua) -> LuaResult<LuaTable<'_>> {
    let exports = lua.create_table()?;

    exports.set(
        "start",
        lua.create_function_mut(|lua, (config,): (LuaValue,)| {
            let config: ModuleConfig = lua.from_value(config)?;
            run_module_with_mlua(module_main, config, lua).map_err(LuaError::external)
        })?,
    )?;

    Ok(exports)
}
