use x86_64::VirtAddr;
use x86_64::structures::tss::TaskStateSegment;
use x86_64::structures::gdt::{GlobalDescriptorTable, Descriptor, SegmentSelector};
use lazy_static::lazy_static;

pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;

lazy_static! {
    static ref TSS: TaskStateSegment = {
        let mut tss = TaskStateSegment::new();
        tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = {
    const STACK_SIZE: usize = 4096 * 5;
    static mut STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];

    // 【核心修改】：在旧版编译器中，即使是 addr_of 也必须放进 unsafe 块
    let stack_start = VirtAddr::from_ptr( { 
        core::ptr::addr_of!(STACK) as *const u8 
    });
    
      let stack_end = stack_start + STACK_SIZE;
      stack_end
    };
        tss
    };

    static ref GDT: (GlobalDescriptorTable, Selectors) = {
        let mut gdt = GlobalDescriptorTable::new();
        
        // 1. 内核代码段
        let code_selector = gdt.add_entry(Descriptor::kernel_code_segment());
        
        // 2. 【核心修复】必须添加内核数据段！
        let data_selector = gdt.add_entry(Descriptor::kernel_data_segment());
        
        // 3. TSS 段 (注意，TSS在64位下占两个槽位)
        let tss_selector = gdt.add_entry(Descriptor::tss_segment(&TSS));

        // 4. 用户态数据段和代码段 (Phase 4 会用到)
        let user_data_selector = gdt.add_entry(Descriptor::user_data_segment());
        let user_code_selector = gdt.add_entry(Descriptor::user_code_segment());

        (gdt, Selectors { 
            code_selector, 
            data_selector, // 增加这个
            tss_selector,
            user_data_selector,
            user_code_selector
        })
    };
}

pub struct Selectors {
    pub code_selector: SegmentSelector,
    pub data_selector: SegmentSelector, // 增加这个
    pub tss_selector: SegmentSelector,
    pub user_data_selector: SegmentSelector,
    pub user_code_selector: SegmentSelector,
}

pub fn init() {
    use x86_64::instructions::tables::load_tss;
    // 引入 SS 和其他数据段寄存器
    use x86_64::instructions::segmentation::{CS, DS, ES, FS, GS, SS, Segment};

    GDT.0.load();
    unsafe {
        CS::set_reg(GDT.1.code_selector);
        
        // 【核心修复】主动刷新 SS 寄存器！
        // 覆盖掉 Bootloader 留下的危险值，保证 iretq 返回时栈段是合法的
        SS::set_reg(GDT.1.data_selector);
        
        // 顺手刷新其他数据段，保证绝对的安全
        DS::set_reg(GDT.1.data_selector);
        ES::set_reg(GDT.1.data_selector);
        // FS 和 GS 在此处不用管

        load_tss(GDT.1.tss_selector);
    }
}