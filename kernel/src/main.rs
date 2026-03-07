// 1. 编译器特性
#![feature(abi_x86_interrupt)]
#![no_std]
#![no_main]

extern crate alloc; 

// 2. 模块声明
#[macro_use]
pub mod serial;      
mod interrupts;      
mod gdt;             
mod mem;             
mod allocator;       
mod task;            
mod syscall; // 【新增】系统调用模块

// 3. 导入
use bootloader_api::{entry_point, BootInfo, BootloaderConfig, config::Mapping};
use core::panic::PanicInfo;
use x86_64::VirtAddr;
use task::{Task, executor::Executor}; 
use futures_util::stream::StreamExt;
use alloc::boxed::Box; 
use x86_64::structures::paging::{Page, PageTableFlags, Size4KiB, Mapper, FrameAllocator};

pub static BOOTLOADER_CONFIG: BootloaderConfig = {
    let mut config = BootloaderConfig::new_default();
    config.mappings.physical_memory = Some(Mapping::Dynamic);
    config.kernel_stack_size = 1024 * 1024; 
    config
};

entry_point!(kernel_main, config = &BOOTLOADER_CONFIG);

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    // ==========================================
    // 第一步：基础初始化
    // ==========================================
    println!("\n\x1b[32;1m[INFO]\x1b[0m Rust Microkernel booting...");

    gdt::init();
    interrupts::init_idt();
    syscall::init(); // 【关键】初始化系统调用硬件配置
    println!("[INFO] GDT, IDT, Syscall-Interface initialized.");

    // ==========================================
    // 第二步：内存管理
    // ==========================================
    let mut frame_allocator = unsafe {
        mem::BootInfoFrameAllocator::init(&boot_info.memory_regions)
    };

    let phys_mem_offset = VirtAddr::new(
        boot_info.physical_memory_offset.into_option().expect("Missing offset")
    );
    let mut mapper = unsafe { mem::init_mapper(phys_mem_offset) };

    allocator::init_heap(&mut mapper, &mut frame_allocator)
        .expect("Failed to initialize kernel heap");
    
    println!("[INFO] Virtual Memory & Heap allocated.");

    // ==========================================
    // 第三步：中断激活
    // ==========================================
    unsafe { interrupts::PICS.lock().initialize(); }
    x86_64::instructions::interrupts::enable(); 
    println!("[INFO] Hardware Interrupts enabled.");

    // ==========================================
    // 第四步：进入用户态 (Ring 3) - 系统调用演示
    // ==========================================
    println!("\x1b[33;1m[INFO]\x1b[0m Loading User Task...");

    let user_code_addr = VirtAddr::new(0x2000_0000); 
    let user_stack_addr = VirtAddr::new(0x2000_1000); 
    let user_stack_top = user_stack_addr + 4096u64;

    unsafe {
        let flags_rw = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;
        
        // 映射用户代码与栈
        let code_frame = frame_allocator.allocate_frame().unwrap();
        let code_page = Page::<Size4KiB>::containing_address(user_code_addr);
        mapper.map_to(code_page, code_frame, flags_rw, &mut frame_allocator).unwrap().flush();

        let stack_frame = frame_allocator.allocate_frame().unwrap();
        let stack_page = Page::<Size4KiB>::containing_address(user_stack_addr);
        mapper.map_to(stack_page, stack_frame, flags_rw, &mut frame_allocator).unwrap().flush();

        // -----------------------------------------------------
        // 【关键】写入带系统调用的用户态机器码 (Shellcode)
        // -----------------------------------------------------
        // 机器码含义：
        // 1. mov rax, 1  (将系统调用号 1 放入 rax)
        // 2. syscall     (触发系统调用，陷入内核)
        // 3. jmp -2      (死循环，防止程序跑飞)
        let code_ptr = user_code_addr.as_mut_ptr::<u8>();
        let shellcode: [u8; 11] = [
            0x48, 0xC7, 0xC0, 0x01, 0x00, 0x00, 0x00, // mov rax, 1
            0x0F, 0x05,                               // syscall
            0xEB, 0xFE                                // jmp self
        ];
        
        for (i, &byte) in shellcode.iter().enumerate() {
            core::ptr::write_volatile(code_ptr.add(i), byte);
        }

        // 移除代码页写权限 (W^X 保护)
        let flags_rx = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
        mapper.update_flags(code_page, flags_rx).unwrap().flush();
    }

    println!("[INFO] User Task ready. Jumping to Ring 3 now...");

    // 启动后台执行器（用于处理键盘等中断）
    let mut executor = Executor::new();
    executor.spawn(Task::new(print_keypresses()));

    // 执行终极跳转
    unsafe {
        gdt::jump_to_user_mode(user_code_addr, user_stack_top);
    }
}

// ==========================================
// 交互任务 (异步)
// ==========================================

async fn print_keypresses() {
    use pc_keyboard::{layouts, DecodedKey, HandleControl, Keyboard, ScancodeSet1};
    let mut scancodes = task::keyboard::ScancodeStream::new();
    let mut keyboard = Keyboard::new(ScancodeSet1::new(), layouts::Us104Key, HandleControl::Ignore);

    while let Some(scancode) = scancodes.next().await {
        if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
            if let Some(key) = keyboard.process_keyevent(key_event) {
                match key {
                    DecodedKey::Unicode(character) => print!("{}", character),
                    DecodedKey::RawKey(key) => print!("{:?}", key),
                }
            }
        }
    }
} 

// ==========================================
// 错误处理
// ==========================================

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("\n\x1b[31;1m[KERNEL PANIC]\x1b[0m {}", info);
    unsafe {
        use x86_64::instructions::port::Port;
        let mut port = Port::new(0xf4);
        port.write(0x11u32);
    }
    loop { x86_64::instructions::hlt(); }
}