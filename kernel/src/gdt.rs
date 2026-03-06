use x86_64::VirtAddr;
use x86_64::structures::tss::TaskStateSegment;
use x86_64::structures::gdt::{GlobalDescriptorTable, Descriptor, SegmentSelector};
use lazy_static::lazy_static;

// 定义双重故障（Double Fault）使用的中断栈索引
// IST 数组共有 7 个槽位，我们占用第 0 个
pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;

lazy_static! {
    // 1. 定义 TSS (Task State Segment)
    // TSS 在 64 位模式下主要用于存放中断栈表 (IST)
    static ref TSS: TaskStateSegment = {
        let mut tss = TaskStateSegment::new();
        
        // 初始化 IST 的第 0 个成员：作为双重故障的专用栈
        tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = {
            // 在内核中开辟一块静态内存作为临时安全栈
            // 注意：生产环境下建议动态分配并加装保护页（Guard Page）
            const STACK_SIZE: usize = 4096 * 5; // 20KB 栈空间
            static mut STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];

            let stack_start = VirtAddr::from_ptr(core::ptr::addr_of!(STACK) as *const u8);
            let stack_end = stack_start + STACK_SIZE;
            
            // 栈是从高地址向低地址增长的，所以传结束地址
            stack_end
        };
        tss
    };

    // 2. 定义 GDT (Global Descriptor Table)
    // 即使在 64 位模式下，GDT 也是切换权限等级和加载 TSS 的必要结构
    static ref GDT: (GlobalDescriptorTable, Selectors) = {
        let mut gdt = GlobalDescriptorTable::new();
        
        // 加载内核代码段描述符
        let code_selector = gdt.add_entry(Descriptor::kernel_code_segment());
        // 加载 TSS 描述符
        let tss_selector = gdt.add_entry(Descriptor::tss_segment(&TSS));
        
        (gdt, Selectors { code_selector, tss_selector })
    };
}

/// 存放 GDT 选中子的结构体，用于后续加载到寄存器
struct Selectors {
    code_selector: SegmentSelector,
    tss_selector: SegmentSelector,
}

/// 初始化并加载 GDT 和 TSS
pub fn init() {
    use x86_64::instructions::tables::load_tss;
    use x86_64::instructions::segmentation::{CS, Segment};

    // 加载 GDT 到 CPU
    GDT.0.load();
    
    unsafe {
        // 更新代码段寄存器 (CS)
        CS::set_reg(GDT.1.code_selector);
        // 加载任务寄存器 (TR)，使 TSS 生效
        load_tss(GDT.1.tss_selector);
    }
}