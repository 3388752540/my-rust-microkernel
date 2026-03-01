#!/bin/bash
set -e

# 1. 显式指定裸机目标编译内核
echo "=== [1/3] Compiling Kernel (no_std) ==="
# 这里加了 --target，强制指定，不依赖任何配置文件
cargo build --package kernel --target x86_64-unknown-none

# 2. 编译启动器 (默认 Linux 目标)
echo "=== [2/3] Compiling Runner (std) ==="
# 这里不加 target，Cargo 就会默认使用 Linux 环境
cargo build --package runner

# 3. 运行
echo "=== [3/3] Booting QEMU ==="
KERNEL_ELF=$(pwd)/target/x86_64-unknown-none/debug/kernel
export KERNEL_ELF=$KERNEL_ELF
cargo run --package runner