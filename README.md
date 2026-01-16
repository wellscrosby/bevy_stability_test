# bevy_stability_test

make sure you have wasm-server-runner

cargo run --release

for optimized build:

cargo build --release
cd target/wasm32-unknown-unknown/release
wasm-opt bevy_stability_test.wasm -o bevy_stability_test.opt.wasm -O3
wasm-server-runner bevy_stability_test.opt.wasm