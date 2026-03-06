// 1. 编译器特性与 Crate 属性
#![feature(abi_x86_interrupt)] 
#![no_std]
#![no_main]

extern crate alloc; // 启用动态内存分配支持

// 2. 模块声明
#[macro_use]
pub mod serial;      // 提供 println! 宏
mod interrupts;      // 中断处理 (IDT, PIC)
mod gdt;             // 段描述符与安全栈 (GDT, TSS)
mod mem;             // 内存分页与物理页帧管理
mod allocator;       // 堆分配器
mod task;            // 异步任务系统 (核心：Task, Executor)

// 3. 导入
use bootloader_api::{entry_point, BootInfo};
use core::panic::PanicInfo;
use x86_64::VirtAddr;
// 注意：这里我们使用高级 Executor 而不是之前的 SimpleExecutor
use task::{Task, executor::Executor}; 

// 4. 定义内核入口
entry_point!(kernel_main);

/// 内核主入口：系统环境初始化 -> 启动异步执行器
fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    // ==========================================
    // 第一步：显示系统启动信息
    // ==========================================
    println!("\n\x1b[32;1m[INFO]\x1b[0m Rust Microkernel booting...");

    // ==========================================
    // 第二步：内核防御与异常处理 (GDT -> IDT)
    // ==========================================
    gdt::init();
    interrupts::init_idt();
    println!("[INFO] CPU Isolation & Exception Handlers ready.");

    // ==========================================
    // 第三步：内存管理系统 (Paging -> Heap)
    // ==========================================
    // A. 物理内存初始化
    let mut frame_allocator = unsafe {
        mem::BootInfoFrameAllocator::init(&boot_info.memory_regions)
    };

    // B. 映射器初始化 (获取物理内存偏移量)
    let phys_mem_offset = VirtAddr::new(
        boot_info.physical_memory_offset.into_option().expect("Missing physical memory offset")
    );
    let mut mapper = unsafe { mem::init_mapper(phys_mem_offset) };

    // C. 堆初始化 (完成后可以使用 Vec, Box, Arc 等)
    allocator::init_heap(&mut mapper, &mut frame_allocator)
        .expect("Failed to initialize kernel heap");
    
    println!("[INFO] Virtual Memory & Heap allocated.");

    // ==========================================
    // 第四步：硬件中断激活 (Timer & Keyboard)
    // ==========================================
    unsafe { interrupts::PICS.lock().initialize(); }
    x86_64::instructions::interrupts::enable(); // 开启中断总开关 (sti)
    println!("[INFO] Hardware Interrupts enabled (System heartbeat started).");

    // ==========================================
    // 第五步：启动异步内核服务 (Async Microkernel)
    // ==========================================
    // 初始化高性能执行器
    let mut executor = Executor::new();
    
    // 任务 A: 演示异步逻辑
    executor.spawn(Task::new(example_async_task()));
    
    // 任务 B: 键盘交互流 (演示中断驱动的异步 IO)
    // 这里我们假设你已经实现了键盘流逻辑，稍后会讲解
    executor.spawn(Task::new(print_keypresses()));

    println!("\x1b[32;1m[SUCCESS]\x1b[0m All kernel subsystems active.");

    // ==========================================
    // 第六步：运行执行器 (此函数永不返回)
    // ==========================================
    // 执行器会在没有任务时自动调用 hlt 指令让 CPU 休息
    executor.run();
}

// ==========================================
// 异步内核服务演示
// ==========================================

/// 异步任务演示：展示非阻塞的 Future 执行
async fn example_async_task() {
    println!("[TASK] Async system test...");
    let result = async { 42 }.await;
    println!("[TASK] Async result confirmed: {}", result);
}

/// 键盘监听任务：通过异步流处理中断触发的数据
async fn print_keypresses() {
    println!("[TASK] Keyboard listener active (Type something in QEMU).");
    // 在下一阶段，我们将实现 ScancodeStream 来让这里真正工作
    /*
    let mut scancodes = ScancodeStream::new();
    while let Some(scancode) = scancodes.next().await {
        println!("[KEY] Received scancode: {:#x}", scancode);
    }
    */
}

// ==========================================
// 系统错误与退出处理
// ==========================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum QemuExitCode {
    Success = 0x10,
    Failed = 0x11,
}

pub fn exit_qemu(exit_code: QemuExitCode) {
    use x86_64::instructions::port::Port;
    unsafe {
        let mut port = Port::new(0xf4);
        port.write(exit_code as u32);
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    // Panic 时强制变红并打印
    println!("\n\x1b[31;1m[KERNEL PANIC]\x1b[0m {}", info);
    
    // 自动退出 QEMU 方便调试，物理机部署时应移除此行
    exit_qemu(QemuExitCode::Failed);
    
    loop {
        x86_64::instructions::hlt();
    }
}