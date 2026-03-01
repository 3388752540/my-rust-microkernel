use std::process::Command;
use std::env;
use std::path::Path;
use bootloader::BiosBoot;

fn main() {
    // 1. 获取内核路径
    let kernel_path = env::var("KERNEL_ELF").expect("请提供 KERNEL_ELF 环境变量");
    
    // 2. 创建输出目录
    let out_dir = Path::new("target/bios_image");
    if !out_dir.exists() {
        std::fs::create_dir_all(out_dir).unwrap();
    }
    // 定义镜像生成的具体路径
    let image_path = out_dir.join("boot.img");

    // 3. 生成镜像
    // ✅ 正确写法： 直接执行，不要接返回值
    let builder = BiosBoot::new(Path::new(&kernel_path));
    builder.create_disk_image(&image_path).expect("镜像打包失败");

    // 4. 启动 QEMU
    println!("启动 QEMU...");
    let mut cmd = Command::new("qemu-system-x86_64");
    
    // ✅ 这里直接使用 image_path.display()
    cmd.arg("-drive").arg(format!("format=raw,file={}", image_path.display()));
    cmd.arg("-serial").arg("stdio");
    cmd.arg("-display").arg("none");
    cmd.arg("-device").arg("isa-debug-exit,iobase=0xf4,iosize=0x04");
    
    let mut child = cmd.spawn().expect("无法启动 QEMU");
    child.wait().unwrap();
}