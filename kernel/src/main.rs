
#![no_std]
#![no_main]

extern crate alloc;

use kernel::{allocator, gdt, mem, println, interrupts};
use kernel::task::{Task, executor::KERNEL_EXECUTOR}; 
use bootloader_api::{entry_point, BootInfo, BootloaderConfig, config::Mapping};
use core::panic::PanicInfo;
use x86_64::VirtAddr;
use x86_64::structures::paging::{Page, PageTableFlags, Size4KiB, Mapper, FrameAllocator};
use alloc::boxed::Box;

mod loader;

// =========================================================
// 1. 强制 ELF 数据对齐 (4096字节)
// 这种写法利用了 Rust 的 unsized coercion，极其优雅
// =========================================================
#[repr(align(4096))]
struct AlignedData<T: ?Sized>(T);

static INIT_ELF: &AlignedData<[u8]> = &AlignedData(*include_bytes!(
    "../../target/x86_64-unknown-none/release/init"
));

pub static BOOTLOADER_CONFIG: BootloaderConfig = {
    let mut config = BootloaderConfig::new_default();
    config.mappings.physical_memory = Some(Mapping::Dynamic);
    config.kernel_stack_size = 1024 * 1024; // 1MB 栈空间
    config
};

entry_point!(kernel_main, config = &BOOTLOADER_CONFIG);

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    println!("\n\x1b[32;1m[INFO]\x1b[0m Rust Microkernel booting...");

    // --- 步骤 1: 基础硬件架构初始化 ---
    kernel::init(); 
    println!("[INFO] Hardware & Syscall MSRs initialized.");

    // --- 步骤 2: 内存分页与堆初始化 ---
    let mut frame_allocator = unsafe {
        mem::BootInfoFrameAllocator::init(&boot_info.memory_regions)
    };
    let phys_mem_offset = VirtAddr::new(
        boot_info.physical_memory_offset.into_option().expect("Missing offset")
    );
    let mut mapper = unsafe { mem::init_mapper(phys_mem_offset) };
    
    allocator::init_heap(&mut mapper, &mut frame_allocator)
        .expect("Heap initialization failed");

    println!("[INFO] Virtual Memory & Kernel Heap ready.");

    // --- 步骤 3: 挂载内核后台异步任务 ---
    {
        let mut executor = KERNEL_EXECUTOR.lock();
        
        // 【核心修改】删除了 print_keypresses！
        // 键盘回显的权力 100% 交给了 Ring 3 的 init 进程
        
        // 任务：系统监控演示任务 (证明内核与用户态在同时运行)
        executor.spawn(Task::new(async {
            // 这个任务在第一次被轮询时会打印，之后完成。
            // 如果你想让它一直运行，可以加个死循环和 sleep
            println!("[KERNEL TASK] Async background monitor started.");
        }));
    }

    // --- 步骤 4: 准备用户态运行环境 ---
    println!("\x1b[33;1m[INFO]\x1b[0m Loading User ELF: 'init'...");

    // A. 建立 32KB 多页用户栈
    let user_stack_base = VirtAddr::new(0x3000_0000);
    let stack_pages = 8; 
    let user_stack_top = user_stack_base + (stack_pages * 4096u64);

    unsafe {
        let stack_flags = PageTableFlags::PRESENT 
                        | PageTableFlags::WRITABLE 
                        | PageTableFlags::USER_ACCESSIBLE;
        
        for i in 0..stack_pages {
            let page_addr = user_stack_base + (i * 4096u64);
            let page = Page::<Size4KiB>::containing_address(page_addr);
            let frame = frame_allocator.allocate_frame().expect("No frames");
            mapper.map_to(page, frame, stack_flags, &mut frame_allocator).unwrap().flush();
        }
    }

    // B. 解析并映射 ELF
    let entry_point = unsafe {
        loader::load_elf(&INIT_ELF.0, &mut mapper, &mut frame_allocator)
    };

    println!("[SUCCESS] ELF loaded. Entry point: {:?}", entry_point);

    // --- 步骤 5: 激活串口硬件中断 (远程交互核心) ---
    unsafe {
        use x86_64::instructions::port::Port;
        let mut ier = Port::<u8>::new(0x3F9);
        ier.write(0x01);
    }
    println!("[INFO] UART Serial hardware unmasked.");

    println!("\x1b[32;1m[READY]\x1b[0m Kernel Tasks and User Process are now merging...");

    // --- 步骤 6: 终极跳转 ---
    unsafe {
        interrupts_enable_and_jump(entry_point, user_stack_top);
    }
}

/// 辅助函数：确保中断控制器全面放行并跳转
unsafe fn interrupts_enable_and_jump(entry: VirtAddr, stack: VirtAddr) -> ! {
    unsafe {
        let mut pics = interrupts::PICS.lock();
        pics.initialize();
        pics.write_masks(0x00, 0x00); // 全部放行
        drop(pics);

        x86_64::instructions::interrupts::enable(); 
        
        gdt::jump_to_user_mode(entry, stack);
    }
}

// ==========================================
// 错误处理
// ==========================================

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("\n\x1b[31;1m[KERNEL PANIC]\x1b[0m {}", info);
    kernel::exit_qemu(kernel::QemuExitCode::Failed);
    loop { x86_64::instructions::hlt(); }
}