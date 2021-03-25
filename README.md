# Tarantool-Rust cross thread communication experiments

## [Async func execution in separate thread](examples/simple)
```bash
cargo build
./target/debug/simple
```

## [Async func execution in separate thread as tarantool lua module](examples/lua)
```bash
tarantoolctl rocks make xtm_test-scm-1.rockspec
echo "require('liblua').cfg()" | RUST_BACKTRACE=1 tarantool
```
