#![no_std]
#![no_main]

use core::panic::PanicInfo;

// ==========================================
// 1. IPC 消息结构定义 (与内核严格对齐)
// ==========================================
#[repr(C)]
#[derive(Debug, Clone, Copy)] // 增加 Clone/Copy 方便操作
pub struct Message {
    pub from: u64,
    pub to: u64,
    pub label: u64,
    pub payload: [u64; 2],
}

// ==========================================
// 2. 系统调用包装函数 (Syscall Wrappers)
// ==========================================

/// SYS_PRINT (ID: 1): 打印单个字符
fn sys_print_char(c: u8) {
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 1,
            in("rdi") c as u64,
            out("rcx") _, 
            out("r11") _, 
            lateout("rax") _, 
        );
    }
}

/// SYS_RECV (ID: 11): 接收消息
/// 如果 options(memory) 报错，我们手动确保编译器不假设内存未变
fn sys_recv(msg: &mut Message) -> u64 {
    let mut res: u64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 11,
            in("rdi") msg as *mut Message as u64,
            lateout("rax") res,
            out("rcx") _,
            out("r11") _,
        );
        // 使用 black_box 强制编译器认为 msg 所在的内存已经发生了变化
        // 这样可以代替 options(memory) 的功能
        core::hint::black_box(msg);
    }
    res
}

/// SYS_YIELD (ID: 12): 让出 CPU
fn sys_yield() {
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 12,
            out("rcx") _, 
            out("r11") _, 
            lateout("rax") _,
        );
    }
}

/// 辅助函数：打印字符串
fn print_str(s: &str) {
    for b in s.as_bytes() {
        sys_print_char(*b);
    }
}

// ==========================================
// 3. 驱动逻辑：扫描码转换 (PS/2 键盘)
// ==========================================
fn scancode_to_char(scancode: u8) -> char {
    match scancode {
        0x1E => 'a', 0x30 => 'b', 0x2E => 'c', 0x20 => 'd', 0x12 => 'e',
        0x21 => 'f', 0x22 => 'g', 0x23 => 'h', 0x17 => 'i', 0x24 => 'j',
        0x25 => 'k', 0x26 => 'l', 0x32 => 'm', 0x31 => 'n', 0x18 => 'o',
        0x19 => 'p', 0x10 => 'q', 0x13 => 'r', 0x1F => 's', 0x14 => 't',
        0x16 => 'u', 0x2F => 'v', 0x11 => 'w', 0x2D => 'x', 0x15 => 'y', 0x2C => 'z',
        0x39 => ' ', 0x1C => '\n',
        _ => '\0',
    }
}

// ==========================================
// 4. 用户进程入口 (Ring 3)
// ==========================================

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let mut msg = Message { from: 0, to: 0, label: 0, payload: [0; 2] };
    
    let mut rsp: u64;
    unsafe { core::arch::asm!("mov {}, rsp", out(reg) rsp); }
    let my_name = if rsp > 0x3500_0000 { "[B]" } else { "[A]" };

    loop {
        // 1. 尝试接收 IPC 消息
       if sys_recv(&mut msg) == 1 {
            if msg.label == 3 {
                let c = msg.payload[0] as u8;
                // 用亮绿色打印回显，方便从一堆 [A][B] 中一眼认出
                print_str("\x1b[32;1m"); 
                sys_print_char(c);
                print_str("\x1b[0m");
            }
        }

        // 2. 降低心跳频率，防止冲刷屏幕
        // 每次循环只打一个点或名字，然后做个超长延迟
        print_str(my_name);
        
        for _ in 0..1_000_000 { // 增加到 1500 万次循环
            unsafe { core::arch::asm!("pause"); } 
        }

        // 3. 【关键修改】不要在用户态写 hlt！
        // 如果想让出 CPU，可以调用 SYS_YIELD (12)
        // 或者暂时直接注释掉 sys_yield(); 靠时钟中断强行切换
    }
}

// ==========================================
// 5. 异常处理
// ==========================================
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        unsafe { core::arch::asm!("pause"); }
    }
}