
#![no_std]
#![no_main]

extern crate alloc;

use kernel::{allocator, gdt, mem, print, println, task, interrupts};
use kernel::task::{Task, executor::Executor};
use bootloader_api::{entry_point, BootInfo, BootloaderConfig, config::Mapping};
use core::panic::PanicInfo;
use x86_64::VirtAddr;
use x86_64::structures::paging::{Page, PageTableFlags, Size4KiB, Mapper, FrameAllocator};
use futures_util::stream::StreamExt;

mod loader;

// =========================================================
// 【核心修复】真正的 4096 字节对齐包装器
// =========================================================
#[repr(align(4096))]
struct AlignedData<T: ?Sized>(T);

// 使用 &... 让编译器自动推导 [u8; N] 中的 N
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
    // 验证对齐情况：现在应该以 000 结尾了
    
    println!("\n\x1b[32;1m[INFO]\x1b[0m Rust Microkernel booting...");

    // 1. 硬件初始化
    kernel::init(); 
    println!("[INFO] Hardware & Syscall MSRs initialized.");

    // 2. 内存与堆初始化
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

    // 3. 准备后台异步任务
    let mut executor = Executor::new();
    executor.spawn(Task::new(print_keypresses()));
    executor.spawn(Task::new(async {
        println!("[TASK] Background system monitor active.");
    }));

    // 4. 加载用户 ELF 程序
    println!("\x1b[33;1m[INFO]\x1b[0m Loading User ELF: 'init'...");

    // A. 为用户栈分配内存 (映射到 0x3000_0000)
    let user_stack_top = VirtAddr::new(0x3000_1000);
    let stack_page = Page::<Size4KiB>::containing_address(user_stack_top - 1u64);
    let stack_frame = frame_allocator.allocate_frame().expect("No frames for stack");
    let stack_flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;
    
    unsafe {
        mapper.map_to(stack_page, stack_frame, stack_flags, &mut frame_allocator)
            .expect("Failed to map user stack")
            .flush();
    }

    // B. 调用 ELF 加载器
    let entry_point = unsafe {
    loader::load_elf(&INIT_ELF.0, &mut mapper, &mut frame_allocator)
};

    println!("[SUCCESS] ELF loaded. Entry point: {:?}", entry_point);
    println!("[INFO] Transitioning to User Mode (Ring 3)...");

    // 5. 开启中断并跳转到用户态
    unsafe {
        interrupts_enable_and_jump(entry_point, user_stack_top);
    }
}

unsafe fn interrupts_enable_and_jump(entry: VirtAddr, stack: VirtAddr) -> ! {
    unsafe {
        interrupts::PICS.lock().initialize();
        x86_64::instructions::interrupts::enable(); 
        gdt::jump_to_user_mode(entry, stack);
    }
}

// ==========================================
// 异步内核服务 (键盘交互)
// ==========================================

async fn print_keypresses() {
    use pc_keyboard::{layouts, DecodedKey, HandleControl, Keyboard, ScancodeSet1};
    let mut scancodes = task::keyboard::ScancodeStream::new();
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
    kernel::exit_qemu(kernel::QemuExitCode::Failed);
    loop { x86_64::instructions::hlt(); }
}