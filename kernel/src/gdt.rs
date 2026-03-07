use x86_64::VirtAddr;
use x86_64::structures::tss::TaskStateSegment;
use x86_64::structures::gdt::{GlobalDescriptorTable, Descriptor, SegmentSelector};
use lazy_static::lazy_static;

// 定义中断栈表 (IST) 索引
pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;

lazy_static! {
    /// 任务状态段 (TSS)
    static ref TSS: TaskStateSegment = {
        let mut tss = TaskStateSegment::new();
        
        // 1. 【关键】设置特权级 0 的栈 (RSP0)
        // 当 CPU 从用户态 (Ring 3) 此时收到中断进入内核态 (Ring 0) 时，
        // 硬件会自动读取这里的地址作为内核栈顶。
        tss.privilege_stack_table[0] = {
            const STACK_SIZE: usize = 4096 * 5;
            static mut STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];
            let stack_start = VirtAddr::from_ptr( core::ptr::addr_of!(STACK) as *const u8);
            stack_start + STACK_SIZE
        };

        // 2. 设置双重故障安全栈 (IST)
        tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = {
            const STACK_SIZE: usize = 4096 * 5;
            static mut STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];
            let stack_start = VirtAddr::from_ptr( core::ptr::addr_of!(STACK)  as *const u8);
            stack_start + STACK_SIZE
        };
        tss
    };

    /// 全局描述符表 (GDT)
    static ref GDT: (GlobalDescriptorTable, Selectors) = {
        let mut gdt = GlobalDescriptorTable::new();
        
        // --- Ring 0 段 (内核) ---
        let kernel_code_selector = gdt.add_entry(Descriptor::kernel_code_segment());
        let kernel_data_selector = gdt.add_entry(Descriptor::kernel_data_segment());
        
        // --- TSS 段 ---
        let tss_selector = gdt.add_entry(Descriptor::tss_segment(&TSS));

        // --- Ring 3 段 (用户态) ---
        // 用户数据段在前，用户代码段在后
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

/// 导出选择子
#[derive(Debug)]
pub struct Selectors {
    pub kernel_code_selector: SegmentSelector,
    pub kernel_data_selector: SegmentSelector,
    pub tss_selector: SegmentSelector,
    pub user_data_selector: SegmentSelector,
    pub user_code_selector: SegmentSelector,
}

/// 获取全局选择子的引用
pub fn get_selectors() -> &'static Selectors {
    &GDT.1
}

/// 初始化 GDT 并加载到硬件
pub fn init() {
    use x86_64::instructions::tables::load_tss;
    use x86_64::instructions::segmentation::{CS, DS, ES, SS, Segment};

    GDT.0.load();
    
    unsafe {
        // 更新段寄存器
        CS::set_reg(GDT.1.kernel_code_selector);
        SS::set_reg(GDT.1.kernel_data_selector);
        DS::set_reg(GDT.1.kernel_data_selector);
        ES::set_reg(GDT.1.kernel_data_selector);
        
        // 加载 TSS
        load_tss(GDT.1.tss_selector);
    }
}

/// 强行跳转到用户态执行指定的函数
/// 该函数手动构建中断返回栈帧 (Stack Frame) 并执行 iretq
pub unsafe fn jump_to_user_mode(user_fn: VirtAddr, user_stack: VirtAddr) -> ! {
    let selectors = get_selectors();

    // 即使函数签名是 unsafe，内部操作也必须包裹在 unsafe 块中
    // 这是 Rust 2024 的新规：unsafe_op_in_unsafe_fn
    unsafe {
        x86_64::instructions::interrupts::disable();

        let ss = selectors.user_data_selector.0 as u64 | 3;
        let cs = selectors.user_code_selector.0 as u64 | 3;

        core::arch::asm!(
            "push {ss}",
            "push {stack_ptr}",
            "push 0x202",
            "push {cs}",
            "push {entry}",
            "iretq",
            ss = in(reg) ss,
            stack_ptr = in(reg) user_stack.as_u64(),
            cs = in(reg) cs,
            entry = in(reg) user_fn.as_u64(),
            options(noreturn)
        );
    }
}