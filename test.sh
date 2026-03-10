#!/bin/bash
# 遇到错误立即停止
set -e

# 1. 参数处理优化
# 如果没有传入参数，默认测 --lib
# 如果传入了 ./test.sh --test basic_boot，$ARGS 会包含完整字符串
if [ $# -eq 0 ]; then
    ARGS="--lib"
else
    ARGS="$@"
fi

echo "=== [1/2] Compiling Kernel with: $ARGS ==="

# 2. 执行编译并获取 JSON 输出
# 增加 --quiet 减少非 JSON 干扰，但保留错误信息
COMPILER_OUTPUT=$(cargo test -p kernel $ARGS --target x86_64-unknown-none --no-run --message-format=json)

# 3. 提取路径的健壮逻辑
# 增加 grep '"reason":"compiler-artifact"' 是为了确保只抓取编译产物行，排除消息行
# tail -n 1 确保拿取最后生成的那个二进制
TEST_BINARY=$(echo "$COMPILER_OUTPUT" | \
    grep '"reason":"compiler-artifact"' | \
    grep '"executable":' | \
    sed -n 's/.*"executable":"\([^"]*\)".*/\1/p' | \
    tail -n 1)

# 4. 路径验证
if [ -z "$TEST_BINARY" ] || [ "$TEST_BINARY" == "null" ]; then
    echo -e "\033[31m错误: 未能找到编译出的测试二进制文件。\033[0m"
    echo "可能的参数错误或编译未产生可执行文件。"
    echo "编译器最后几行输出:"
    echo "$COMPILER_OUTPUT" | tail -n 10
    exit 1
fi

echo -e "\033[32m找到测试镜像: $TEST_BINARY\033[0m"

# 5. 调用启动器
echo "=== [2/2] Booting QEMU ==="
export KERNEL_ELF=$TEST_BINARY

# 运行 runner
cargo run --package runner --release