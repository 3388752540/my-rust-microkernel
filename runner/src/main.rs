use std::env;
use std::path::Path;
use std::process::Command;
use bootloader::BiosBoot; // 确保 runner/Cargo.toml 里有 bootloader = "0.11"

fn main() {
    // 1. 获取内核路径 (由 run.sh 脚本传入)
    let kernel_path = env::var("KERNEL_ELF").expect("请提供 KERNEL_ELF 环境变量");
    
    // 2. 创建输出目录
    let out_dir = Path::new("target/bios_image");
    if !out_dir.exists() {
        std::fs::create_dir_all(out_dir).unwrap();
    }
    // 定义镜像生成的具体路径
    let image_path = out_dir.join("boot.img");

    // 3. 生成可引导镜像
    let builder = BiosBoot::new(Path::new(&kernel_path));
    builder.create_disk_image(&image_path).expect("镜像打包失败");

    // 4. 启动 QEMU
    println!("启动 QEMU...");
    let mut cmd = Command::new("qemu-system-x86_64");
    
    // 挂载生成的磁盘镜像
    cmd.arg("-drive").arg(format!("format=raw,file={}", image_path.display()));
    
    // 测试退出的设备支持
    cmd.arg("-device").arg("isa-debug-exit,iobase=0xf4,iosize=0x04");
    
    // ========================================================
    // 【核心修复】：远程 SSH 开发的完美终端配置
    // ========================================================
    // 1. -nographic: 告诉 QEMU 彻底禁用图形窗口（VGA）
    cmd.arg("-nographic");
    
    // 2. -serial mon:stdio: 
    //    将当前终端的标准输入(键盘)和标准输出(屏幕)复用，
    //    并直接连接到虚拟机的 COM1 串口 (0x3F8)。
    //    【注意】：不要再写其他的 -serial 或 -display 参数了！
    cmd.arg("-serial").arg("mon:stdio");
    
    // 启动子进程并等待其运行结束
    let mut child = cmd.spawn().expect("无法启动 QEMU");
    child.wait().unwrap();
}