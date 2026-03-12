#![no_std]
#![no_main]


extern crate alloc;

use kernel::{allocator, gdt, mem, println, print, interrupts};
use kernel::task::{Task, TaskId, TASK_REGISTRY, executor::KERNEL_EXECUTOR}; 
use bootloader_api::{entry_point, BootInfo, BootloaderConfig, config::Mapping};
use core::panic::PanicInfo;
use core::sync::atomic::Ordering;
use x86_64::VirtAddr;
use x86_64::structures::paging::{Page, PageTableFlags, Size4KiB, Mapper, FrameAllocator};
use alloc::sync::Arc;
use futures_util::stream::StreamExt;

mod loader;

// =========================================================
// 1. 强制 ELF 数据对齐 (4096字节)
// =========================================================
#[repr(align(4096))]
struct AlignedData<T: ?Sized>(T);

static INIT_ELF: &AlignedData<[u8]> = &AlignedData(*include_bytes!(
    "../../target/x86_64-unknown-none/release/init"
));

pub static BOOTLOADER_CONFIG: BootloaderConfig = {
    let mut config = BootloaderConfig::new_default();
    config.mappings.physical_memory = Some(Mapping::Dynamic);
    config.kernel_stack_size = 1024 * 1024;
    config
};

entry_point!(kernel_main, config = &BOOTLOADER_CONFIG);

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    println!("\n\x1b[32;1m[INFO]\x1b[0m Rust Microkernel booting...");

    // --- 步骤 1: 基础硬件架构初始化 ---
    // 初始化 GDT, IDT, Syscall MSRs, 并对 PIC 进行重映射
    kernel::init(); 
    println!("[INFO] Hardware protection & Syscall MSRs ready.");

    // --- 步骤 2: 内存管理初始化 ---
    let mut frame_allocator = unsafe {
        mem::BootInfoFrameAllocator::init(&boot_info.memory_regions)
    };
    let phys_mem_offset = VirtAddr::new(
        boot_info.physical_memory_offset.into_option().expect("Missing offset")
    );
    let mut mapper = unsafe { mem::init_mapper(phys_mem_offset) };
    
    // 初始化内核堆 (Heap)，此后 Vec/Arc/Box 可用
    allocator::init_heap(&mut mapper, &mut frame_allocator).expect("Heap failed");
    println!("[INFO] Virtual Memory & Kernel Heap ready.");

    // --- 步骤 3: 准备用户态程序镜像 ---
    let entry_point = unsafe {
        loader::load_elf(&INIT_ELF.0, &mut mapper, &mut frame_allocator)
    };

    // --- 步骤 4: 创建两个相互隔离的用户进程 (Phase 7 核心) ---
    println!("\x1b[33;1m[INFO]\x1b[0m Spawning Process A and B (Ring 3)...");
    
    // 进程 A: 映射 0x3000_0000 起始的栈 (32KB)
    let stack_a_top = setup_user_stack(0x3000_0000, 8, &mut mapper, &mut frame_allocator);
    let process_a = Task::create_user_process(entry_point, stack_a_top);

    // 进程 B: 映射 0x4000_0000 起始的栈 (32KB)
    let stack_b_top = setup_user_stack(0x4000_0000, 8, &mut mapper, &mut frame_allocator);
    let process_b = Task::create_user_process(entry_point, stack_b_top);

    // 注册到全局调度表 (TASK_LIST)
    {
        let mut list = interrupts::TASK_LIST.lock();
        list.push(Arc::clone(&process_a));
        list.push(Arc::clone(&process_b));
    }

    // --- 步骤 5: 挂载内核后台异步任务 ---
    {
        let mut executor = KERNEL_EXECUTOR.lock();
        executor.spawn(Task::new(async {
            println!("[KERNEL TASK] Async system monitor active.");
        }));
        // 如果有 PS/2 键盘，开启此任务
        // executor.spawn(Task::new(print_keypresses()));
    }

    // --- 步骤 6: 【工业级修复】深度开启串口硬件中断 ---
    // 这不仅要在串口芯片上开启中断，还要在 PIC 控制器上解除屏蔽
    unsafe {
        use x86_64::instructions::port::Port;
        let base = 0x3F8;
        // 1. 串口芯片配置 (UART 16550)
        Port::new(base + 1).write(0x01u8); // IER: 开启接收中断
        Port::new(base + 2).write(0xC7u8); // FCR: 开启并清空 FIFO
        Port::new(base + 4).write(0x0Bu8); // MCR: 必须开启 Bit 3 (OUT2) 才能触发 IRQ

        // 2. PIC 屏蔽字配置 (工业级动态开启)
        // 解除 IRQ 0 (时钟), IRQ 1 (键盘), IRQ 4 (串口) 的屏蔽
        // 0x21 是主片 IMR 端口。计算方式：mask & !(1<<0 | 1<<1 | 1<<4)
        // 原本是 b8 (1011 1000)，我们需要设为 a8 (1010 1000)
        let mut master_mask = Port::<u8>::new(0x21);
        let mask = master_mask.read();
        master_mask.write(mask & !( (1 << 0) | (1 << 1) | (1 << 4) ));
    }
    println!("[INFO] UART Serial hardware fully unmasked (IRQ 4 active).");

    println!("\x1b[32;1m[READY]\x1b[0m Preemptive Multitasking starting...");

    // --- 步骤 7: 【终极点火】弹出进程 A 的现场并开启抢占 ---
    unsafe {
        // 更新 TSS，为进程 A 第一次被时钟打断做好准备
        gdt::set_interrupt_stack(VirtAddr::new(process_a.kernel_stack_top));

        // 记录当前运行的 PID 到全局变量，供 IPC 识别
        kernel::task::CURRENT_TASK_ID.store(process_a.id.0, Ordering::SeqCst);

        let first_rsp = process_a.kernel_rsp.load(Ordering::SeqCst);

        // 只有在这里执行开启中断，内核才进入“实时状态”
        x86_64::instructions::interrupts::enable(); 

        // 执行手动上下文切换，跳进进程 A
        // 这一步之后，main 函数的生命周期就正式交接给了调度器
        core::arch::asm!(
            "mov rsp, {rsp}",
            "pop r15", "pop r14", "pop r13", "pop r12",
            "pop r11", "pop r10", "pop r9", "pop r8",
            "pop rbp", "pop rsi", "pop rdi", "pop rdx",
            "pop rcx", "pop rbx", "pop rax",
            "iretq",
            rsp = in(reg) first_rsp,
            options(noreturn)
        );
    }
}

/// 辅助函数：为用户进程映射独立的物理页作为栈
fn setup_user_stack(
    base_addr: u64, 
    pages: u64, 
    mapper: &mut impl Mapper<Size4KiB>, 
    frame_allocator: &mut impl FrameAllocator<Size4KiB>
) -> VirtAddr {
    let stack_base = VirtAddr::new(base_addr);
    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;
    for i in 0..pages {
        let page = Page::containing_address(stack_base + (i * 4096));
        let frame = frame_allocator.allocate_frame().expect("No physical frames for stack");
        unsafe { mapper.map_to(page, frame, flags, frame_allocator).unwrap().flush(); }
    }
    // 返回栈顶地址 (x86 栈向下增长，所以返回最高地址)
    stack_base + (pages * 4096)
}

/// 演示用的键盘流任务 (仅针对 PS/2 键盘)
async fn print_keypresses() {
    use pc_keyboard::{layouts, DecodedKey, HandleControl, Keyboard, ScancodeSet1};
    let mut scancodes = kernel::task::keyboard::ScancodeStream::new();
    let mut keyboard = Keyboard::new(ScancodeSet1::new(), layouts::Us104Key, HandleControl::Ignore);

    while let Some(scancode) = scancodes.next().await {
        if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
            if let Some(key) = keyboard.process_keyevent(key_event) {
                match key {
                    DecodedKey::Unicode(c) => print!("{}", c),
                    DecodedKey::RawKey(k) => print!("{:?}", k),
                }
            }
        }
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("\n\x1b[31;1m[KERNEL PANIC]\x1b[0m {}", info);
    // 强制关闭 QEMU 方便调试
    kernel::exit_qemu(kernel::QemuExitCode::Failed);
    loop { x86_64::instructions::hlt(); }
}