#!/bin/bash
set -e

# 1. 编译内核
echo "=== [1/2] Compiling Kernel ==="
cargo build --package kernel --target x86_64-unknown-none --release

# 2. 运行启动器
echo "=== [2/2] Booting QEMU ==="
KERNEL_ELF=$(pwd)/target/x86_64-unknown-none/release/kernel
export KERNEL_ELF=$KERNEL_ELF

cargo run --package runner --release