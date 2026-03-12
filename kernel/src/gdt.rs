use x86_64::VirtAddr;
use x86_64::structures::tss::TaskStateSegment;
use x86_64::structures::gdt::{GlobalDescriptorTable, Descriptor, SegmentSelector};
use lazy_static::lazy_static;
use core::arch::naked_asm;

pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;

// ==========================================
// 1. 静态 TSS 定义 (符合硬件固定地址要求)
// ==========================================
// 使用 static mut 是为了让 GDT 能够获得它的 'static 引用。
// 虽然 static mut 在 Rust 2024 中受限，但配合 addr_of_mut 是底层开发的标准做法。
static mut TSS: TaskStateSegment = TaskStateSegment::new();

lazy_static! {
    static ref GDT: (GlobalDescriptorTable, Selectors) = {
        let mut gdt = GlobalDescriptorTable::new();
        
        // --- Ring 0 段 ---
        let kernel_code_selector = gdt.add_entry(Descriptor::kernel_code_segment());
        let kernel_data_selector = gdt.add_entry(Descriptor::kernel_data_segment());
        
        // --- TSS 段 ---
        // 关键修复：直接引用静态变量，不再使用 Mutex 产生临时值
        let tss_selector = gdt.add_entry(Descriptor::tss_segment(unsafe { &*core::ptr::addr_of!(TSS) }));

        // --- Ring 3 段 ---
        let user_data_selector = gdt.add_entry(Descriptor::user_data_segment());
        let user_code_selector = gdt.add_entry(Descriptor::user_code_segment());

        (gdt, Selectors { 
            kernel_code_selector, 
            kernel_data_selector,
            tss_selector,
            user_data_selector,
            user_code_selector
        })
    };
}

pub struct Selectors {
    pub kernel_code_selector: SegmentSelector,
    pub kernel_data_selector: SegmentSelector,
    pub tss_selector: SegmentSelector,
    pub user_data_selector: SegmentSelector,
    pub user_code_selector: SegmentSelector,
}

pub fn get_selectors() -> &'static Selectors {
    &GDT.1
}

pub fn init() {
    use x86_64::instructions::tables::load_tss;
    use x86_64::instructions::segmentation::{CS, DS, ES, SS, Segment};

    // 初始化 TSS 内部的栈（可以在这里做，也可以在 lazy_static 做）
    unsafe {
        let tss = &mut *core::ptr::addr_of_mut!(TSS);
        
        // 设置 RSP0
        const STACK_SIZE: usize = 4096 * 5;
        static mut RSP0_STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];
        tss.privilege_stack_table[0] = VirtAddr::from_ptr(core::ptr::addr_of!(RSP0_STACK)) + STACK_SIZE;

        // 设置 IST0
        static mut IST0_STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];
        tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = 
            VirtAddr::from_ptr(core::ptr::addr_of!(IST0_STACK)) + STACK_SIZE;
    }

    GDT.0.load();
    unsafe {
        CS::set_reg(GDT.1.kernel_code_selector);
        SS::set_reg(GDT.1.kernel_data_selector);
        DS::set_reg(GDT.1.kernel_data_selector);
        ES::set_reg(GDT.1.kernel_data_selector);
        load_tss(GDT.1.tss_selector);
    }
}

/// 动态更新 TSS 中的内核栈
pub unsafe fn set_interrupt_stack(stack_top: VirtAddr) {
    let tss = unsafe { &mut *core::ptr::addr_of_mut!(TSS) };
    tss.privilege_stack_table[0] = stack_top;
}

/// 跳转到用户态
/// 参数说明：rdi = user_fn, rsi = user_stack
#[unsafe(naked)]
pub unsafe extern "C" fn jump_to_user_mode(user_fn: VirtAddr, user_stack: VirtAddr) -> ! {
    // 关键修复：使用 naked_asm!。
    // 在 naked 函数中不能使用命名参数（如 {cs}），必须通过寄存器手动处理。
    // 我们假设调用者已经把 RIP 放在了 rdi，RSP 放在了 rsi。
    
        naked_asm!(
            "cli",
            // 我们需要 User CS (0x23) 和 User DS (0x1b)
            // 在我们的 GDT 布局中：
            // 0: Null, 8: KCode, 16: KData, 24: TSS(16byte), 40: UData, 48: UCode
            // 因此 User Data = 40 | 3 = 43 (0x2b), User Code = 48 | 3 = 51 (0x33)
            "mov rax, 0x2b", // User Data Selector (RPL 3)
            "mov rbx, rsi",  // User Stack Pointer (arg rsi)
            "mov rcx, 0x202",// RFLAGS (IF=1)
            "mov rdx, 0x33", // User Code Selector (RPL 3)
            "mov rbp, rdi",  // User Entry Point (arg rdi)

            "push rax",      // SS
            "push rbx",      // RSP
            "push rcx",      // RFLAGS
            "push rdx",      // CS
            "push rbp",      // RIP
            "iretq",
        );
    
}