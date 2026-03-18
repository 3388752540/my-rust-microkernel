#![no_std]
#![no_main]

use core::panic::PanicInfo;

// ==========================================
// 1. 协议与数据结构
// ==========================================

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Message {
    pub from: u64,
    pub to: u64,
    pub label: u64,
    pub payload: [u64; 2],
}

const SERVICE_NAME_SERIAL: u64 = 0x20204C4149524553; // "SERIAL  "

// ==========================================
// 2. 系统调用封装
// ==========================================

fn sys_print_char(c: u8) {
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 1, in("rdi") c as u64,
            out("rcx") _, out("r11") _, lateout("rax") _,
        );
    }
}

fn sys_exit() -> ! {
    unsafe {
        core::arch::asm!("syscall", in("rax") 2, out("rcx") _, out("r11") _, lateout("rax") _);
    }
    loop {}
}

fn sys_recv(msg: &mut Message) -> u64 {
    let mut res: u64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 11,
            in("rdi") msg as *mut _ as u64,
            lateout("rax") res,
            out("rcx") _, out("r11") _,
            
        );
         core::hint::black_box(msg);
    }
    res
}

fn sys_yield() {
    unsafe {
        core::arch::asm!("syscall", in("rax") 12, out("rcx") _, out("r11") _, lateout("rax") _);
    }
}

fn sys_reg_service(name: u64) {
    unsafe {
        core::arch::asm!("syscall", in("rax") 14, in("rdi") name, out("rcx") _, out("r11") _, lateout("rax") _);
    }
}

fn sys_get_service_pid(name: u64) -> u64 {
    let mut res: u64;
    unsafe {
        core::arch::asm!("syscall", in("rax") 15, in("rdi") name, lateout("rax") res, out("rcx") _, out("r11") _);
    }
    res
}

// ==========================================
// 3. 辅助功能
// ==========================================

fn print_str(s: &str) {
    for b in s.as_bytes() { sys_print_char(*b); }
}

// ==========================================
// 4. 程序入口 (Ring 3)
// ==========================================

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let mut rsp: u64;
    unsafe { core::arch::asm!("mov {}, rsp", out(reg) rsp); }
    let is_process_b = rsp > 0x3500_0000;

    if is_process_b {
        // --- 进程 B 逻辑：故意犯错的“倒霉蛋” ---
        print_str("\x1b[33m[Process B] Started. Target: Search & Die.\x1b[0m\n");
        
        // 等待 A 注册服务
        for _ in 0..5_000_000 { unsafe { core::arch::asm!("pause"); } }

        let pid = sys_get_service_pid(SERVICE_NAME_SERIAL);
        if pid != u64::MAX {
            print_str("[Process B] Service SERIAL is at PID ");
            sys_print_char((pid as u8) + b'0');
            print_str("\n");
        }

        print_str("\x1b[31;1m[Process B] Executing 'hlt' to test Fault Isolation...\x1b[0m\n");
        
        // 【关键点】：这条指令在 Ring 3 会触发 GPF。
        // 如果内核的 gpf_handler 修复了，系统会立刻切回进程 A。
        unsafe { core::arch::asm!("hlt"); }
        
        sys_exit();
    } else {
        // --- 进程 A 逻辑：勤劳的 Echo Server ---
        sys_reg_service(SERVICE_NAME_SERIAL);
        print_str("\x1b[32;1m[Process A] SERIAL server online. Type something!\x1b[0m\n> ");

        let mut msg = Message { from: 0, to: 0, label: 0, payload: [0; 2] };

        loop {
            // 1. 尝试收信
            if sys_recv(&mut msg) == 1 {
                if msg.label == 3 { // 串口输入
                    let c = msg.payload[0] as u8;
                    // 回显字符，使用绿色加粗
                    print_str("\x1b[32;1m");
                    sys_print_char(c);
                    print_str("\x1b[0m");
                }
            } else {
                // 2. 没信时，不要在这做长循环！
                // 直接 yield 把时间片给内核，等待中断唤醒
                sys_yield();
            }
        }
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop { unsafe { core::arch::asm!("pause"); } }
}