#!/bin/bash
set -e  # 一旦出错立即停止

echo "=== [1/3] Compiling User App (init) ==="
# 先编译用户态程序，生成二进制文件
cd user/init
cargo build --release --target x86_64-unknown-none
cd ../..

echo "=== [2/3] Compiling Kernel ==="
# 此时 target 目录下已有 init 文件，内核编译可以通过
cargo build --package kernel --release --target x86_64-unknown-none

echo "=== [3/3] Booting QEMU ==="
# 设置内核路径并运行启动器
KERNEL_ELF=$(pwd)/target/x86_64-unknown-none/release/kernel
export KERNEL_ELF=$KERNEL_ELF

cargo run --package runner --release