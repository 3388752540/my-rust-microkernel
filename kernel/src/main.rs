// 1. 编译器特性与 Crate 属性
#![feature(abi_x86_interrupt)] 
#![no_std]
#![no_main]

extern crate alloc; // 启用动态内存分配支持

// 2. 模块声明
#[macro_use]
pub mod serial;      // 提供 println! 和 print! 宏
mod interrupts;      // 中断处理 (IDT, PIC)
mod gdt;             // 段描述符与安全栈 (GDT, TSS)
mod mem;             // 内存分页与物理页帧管理
mod allocator;       // 堆分配器
mod task;            // 异步任务系统 (Task, Executor)

// 3. 导入
use bootloader_api::{entry_point, BootInfo, BootloaderConfig, config::Mapping};
use core::panic::PanicInfo;
use x86_64::VirtAddr;
use task::{Task, executor::Executor}; 
use futures_util::stream::StreamExt; // 必须导入：提供 .next().await 功能

pub static BOOTLOADER_CONFIG: BootloaderConfig = {
    let mut config = BootloaderConfig::new_default();
    // 1. 开启物理内存映射
    config.mappings.physical_memory = Some(Mapping::Dynamic);
    
    // 2. 【新增】扩充内核栈大小
    // 默认通常只有 80KB 左右，我们将其设置为 512KB
    // 这能有效防止异步任务深度嵌套导致的 DOUBLE FAULT
    config.kernel_stack_size = 1024 * 1024; 
    
    config
};

// 3. 关联配置
entry_point!(kernel_main, config = &BOOTLOADER_CONFIG);


// 内核主函数：系统环境初始化 -> 启动异步执行器
fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    // ==========================================
    // 第一步：基础硬件与打印初始化
    // ==========================================
    println!("\n\x1b[32;1m[INFO]\x1b[0m Rust Microkernel booting...");

    // 初始化 GDT/TSS 和 IDT
    gdt::init();
    interrupts::init_idt();
    println!("[INFO] GDT, TSS, IDT initialized.");

    // ==========================================
    // 第二步：内存管理系统初始化
    // ==========================================
    let mut frame_allocator = unsafe {
        mem::BootInfoFrameAllocator::init(&boot_info.memory_regions)
    };

    let phys_mem_offset = VirtAddr::new(
        boot_info.physical_memory_offset.into_option().expect("Missing physical memory offset")
    );
    let mut mapper = unsafe { mem::init_mapper(phys_mem_offset) };

    allocator::init_heap(&mut mapper, &mut frame_allocator)
        .expect("Failed to initialize kernel heap");
    
    println!("[INFO] Virtual Memory & Heap allocated.");

    // ==========================================
    // 第三步：硬件中断激活
    // ==========================================
    unsafe { interrupts::PICS.lock().initialize(); }
    x86_64::instructions::interrupts::enable(); 
    println!("[INFO] Hardware Interrupts enabled.");

    // ==========================================
    // 第四步：启动异步内核服务
    // ==========================================
    let mut executor = Executor::new();
    
    // 任务 A: 演示异步逻辑
    executor.spawn(Task::new(example_async_task()));
    
    // 任务 B: 键盘交互流任务 (正式补全)
    // 这一行调用了 ScancodeStream::new()，会解决之前的 unused 警告
    executor.spawn(Task::new(print_keypresses()));

    println!("\x1b[32;1m[SUCCESS]\x1b[0m All systems active. Type in QEMU window!");

    // ==========================================
    // 第五步：运行执行器
    // ==========================================
    executor.run();
}

// ==========================================
// 异步内核服务 (Async Handlers)
// ==========================================

/// 演示任务
async fn example_async_task() {
    println!("[TASK] Async test task started...");
    let result = async { 10 + 32 }.await;
    println!("[TASK] Async result: {}", result);
}

/// 核心任务：键盘监听器
/// 利用异步 Stream 机制处理按键，这是微内核处理 I/O 的标准模式
async fn print_keypresses() {
    use pc_keyboard::{layouts, DecodedKey, HandleControl, Keyboard, ScancodeSet1};

    // 1. 获取异步键盘扫描码流
    let mut scancodes = task::keyboard::ScancodeStream::new();
    
    // 2. 初始化键盘解析器 (美式布局，扫描码集 1)
    let mut keyboard = Keyboard::new(
        ScancodeSet1::new(),      // 使用 ::new() 方法
        layouts::Us104Key,        // 直接写名字，不需要 {}
        HandleControl::Ignore
    );


    println!("[TASK] Keyboard listener active.");

    // 3. 异步循环：当且仅当有按键中断时，此循环才会继续，否则任务 Pending
    while let Some(scancode) = scancodes.next().await {
        // 将原始字节解码为键盘事件
        if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
            // 将事件转换为具体的字符
            if let Some(key) = keyboard.process_keyevent(key_event) {
                match key {
                    // 如果是普通字符，直接通过串口打印出来
                    DecodedKey::Unicode(character) => print!("{}", character),
                    // 如果是功能键，打印其调试名称
                    DecodedKey::RawKey(key) => print!("{:?}", key),
                }
            }
        }
    }
} 

/* fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    // 1. 基础初始化
    gdt::init();
    interrupts::init_idt();
    
    // 2. 内存初始化
    let mut frame_allocator = unsafe {
        mem::BootInfoFrameAllocator::init(&boot_info.memory_regions)
    };
    let phys_mem_offset = VirtAddr::new(
        boot_info.physical_memory_offset.into_option().unwrap()
    );
    let mut mapper = unsafe { mem::init_mapper(phys_mem_offset) };
    allocator::init_heap(&mut mapper, &mut frame_allocator).expect("Heap init failed");

    println!("[TEST] Heap and IDT are ready.");

    // ==========================================
    // 关键测试 A：测试 println! 是否稳定
    // ==========================================
    for i in 0..100 {
        println!("[TEST] Loop count: {}", i);
    }

    // ==========================================
    // 关键测试 B：测试异常处理是否真的有效
    // ==========================================
    println!("[TEST] Triggering a safe breakpoint...");
    x86_64::instructions::interrupts::int3(); 
    println!("[TEST] If you see this, IDT is 100% correct.");

    loop { x86_64::instructions::hlt(); }
} */

// ==========================================
// 系统维护与错误处理
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
    println!("\n\x1b[31;1m[KERNEL PANIC]\x1b[0m {}", info);
    exit_qemu(QemuExitCode::Failed);
    loop {
        x86_64::instructions::hlt();
    }
}