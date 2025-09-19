# XTM Benchmark

### Requirements
- Rust, cargo
- Tarantool
- g++

### Build
```bash
make build
make install
```

### Run benchmark
```bash
tarantool init.lua
```

### Results
- 6-core Intel i5-8400
- tarantool 2.8.0
- rust 1.53 nightly
- linux 5.11.16

### 1 thread, 1 worker
```
iterations: 10000000
elapsed: 4451806528ns
avg rps: 2246279
p50: 846ns
p90: 910ns
p99: 1012ns
mean: 854ns
min: 742ns
max: 676603ns
```

### 1 thread, 2 workers
```
iterations: 10000000
elapsed: 9170847683ns
avg rps: 1090411
p50: 854ns
p90: 957ns
p99: 1033ns
mean: 1789ns
min: 785ns
max: 3687599ns
```

### 6 threads, 1 worker
```
iterations: 10000000
elapsed: 10548284094ns
avg rps: 948021
p50: 960ns
p90: 1197ns
p99: 1521ns
mean: 1020ns
min: 754ns
max: 3136159ns
```

### 6 threads, 3 workers
```
iterations: 10000000
elapsed: 10548284094ns
avg rps: 948021
p50: 960ns
p90: 1197ns
p99: 1521ns
mean: 1020ns
min: 754ns
max: 3136159ns
```

### 6 threads, 6 workers
```
iterations: 10000000
elapsed: 4249654009ns
avg rps: 2353132
p50: 1851ns
p90: 1993ns
p99: 2609ns
mean: 2474ns
min: 753ns
max: 10027583ns
```