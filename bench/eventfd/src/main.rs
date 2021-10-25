use std::{convert::TryInto, time};
use tokio::runtime::Builder;
use xtm_rust::eventfd;

fn main() {
    println!("threads_nonblocking:");
    threads_nonblocking(1_000_000);

    println!("tasks:");
    tasks(1_000_000);
}

fn threads_nonblocking(iterations: u64) {
    let efd1 = eventfd::EventFd::new(0, false).unwrap();
    let efd2 = eventfd::EventFd::new(0, false).unwrap();

    let rt1 = Builder::new_current_thread().enable_all().build().unwrap();
    let rt2 = Builder::new_current_thread().enable_all().build().unwrap();

    let efd1_clone = efd1.try_clone().unwrap();
    let efd2_clone = efd2.try_clone().unwrap();

    let begin = time::Instant::now();
    let thread1 = std::thread::spawn(move || {
        rt1.block_on(async {
            let input: eventfd::AsyncEventFd = efd1_clone.try_into().unwrap();
            let output: eventfd::AsyncEventFd = efd2_clone.try_into().unwrap();

            output.write(1).await.unwrap();
            loop {
                let val = input.read().await.unwrap();
                if val >= iterations {
                    break;
                }

                output.write(val + 1).await.unwrap();
            }
        });
    });
    let thread2 = std::thread::spawn(move || {
        rt2.block_on(async {
            let input: eventfd::AsyncEventFd = efd2.try_into().unwrap();
            let output: eventfd::AsyncEventFd = efd1.try_into().unwrap();

            loop {
                let val = input.read().await.unwrap();
                let next_val = val + 1;
                output.write(next_val).await.unwrap();
                if next_val >= iterations {
                    break;
                }
            }
        })
    });

    thread1.join().unwrap();
    thread2.join().unwrap();

    let elapsed = begin.elapsed();
    let rps = iterations as f64 / elapsed.as_secs_f64();
    println!("rps: {}", rps);
}

fn tasks(iterations: u64) {
    let efd1 = eventfd::EventFd::new(0, false).unwrap();
    let efd2 = eventfd::EventFd::new(0, false).unwrap();

    let rt = Builder::new_current_thread().enable_all().build().unwrap();

    let begin = time::Instant::now();
    rt.block_on(async move {
        let efd1: eventfd::AsyncEventFd = efd1.try_into().unwrap();
        let efd2: eventfd::AsyncEventFd = efd2.try_into().unwrap();

        let input = efd1.try_clone().unwrap();
        let output = efd2.try_clone().unwrap();
        let task1 = tokio::spawn(async move {
            output.write(1).await.unwrap();
            loop {
                let val = input.read().await.unwrap();
                if val >= iterations {
                    break;
                }

                output.write(val + 1).await.unwrap();
            }
        });

        let input = efd2;
        let output = efd1;
        let task2 = tokio::spawn(async move {
            loop {
                let val = input.read().await.unwrap();
                let next_val = val + 1;
                output.write(next_val).await.unwrap();
                if next_val >= iterations {
                    break;
                }
            }
        });

        task1.await.unwrap();
        task2.await.unwrap();
    });

    let elapsed = begin.elapsed();
    let rps = iterations as f64 / elapsed.as_secs_f64();
    println!("rps: {}", rps);
}
