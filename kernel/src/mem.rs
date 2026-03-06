use bootloader_api::info::{MemoryRegions, MemoryRegionKind};
use x86_64::{
    structures::paging::{FrameAllocator, PhysFrame, Size4KiB},
    PhysAddr,
};
use x86_64::VirtAddr;
use x86_64::structures::paging::OffsetPageTable;
use x86_64::registers::control::Cr3;

/// 物理页帧分配器：负责管理 4KB 大小的物理内存块
pub struct BootInfoFrameAllocator {
    // 引导程序传给我们的内存区域列表
    memory_regions: &'static MemoryRegions,
    // 一个计数器，记录下一个要分配的可用页帧索引，步进分配器
    next: usize,
}

impl BootInfoFrameAllocator {
    /// 根据内存区域列表创建一个分配器
    /// 这里的 unsafe 是因为调用者必须保证传入的 memory_regions 是正确的
    pub unsafe fn init(memory_regions: &'static MemoryRegions) -> Self {
        BootInfoFrameAllocator {
            memory_regions,
            next: 0,
        }
    }

    /// 一个辅助方法：返回内存映射中所有“可用（Usable）”区域的物理页帧迭代器
    fn usable_frames(&self) -> impl Iterator<Item = PhysFrame> {
        // 获取所有的内存区域
        self.memory_regions.iter()
            // 过滤出那些被标记为“可用”的区域（避开引导程序、硬件保留区等）
            .filter(|r| r.kind == MemoryRegionKind::Usable)
            // 将每个区域转换为其包含的地址范围
            .map(|r| r.start..r.end)
            // 将地址范围转换为每 4KB 一个步长的迭代器
            .flat_map(|r| r.step_by(4096))
            // 将地址转换为 PhysFrame 类型
            .map(|addr| PhysFrame::containing_address(PhysAddr::new(addr)))
    }
}

// 为我们的结构体实现 x86_64 库定义的 FrameAllocator 特性
unsafe impl FrameAllocator<Size4KiB> for BootInfoFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        // 从 usable_frames 迭代器中取出第 next 个页帧
        let frame = self.usable_frames().nth(self.next);
        // 增加计数器，下次分配下一个
        self.next += 1;
        frame
    }
}

pub unsafe fn init_mapper(physical_memory_offset: VirtAddr) -> OffsetPageTable<'static> {
    // 从 CR3 寄存器读取当前活动的 4 级页表物理帧
    let (level_4_table_frame, _) = Cr3::read();

    let phys = level_4_table_frame.start_address();
    // 关键：计算页表在虚拟内存中的地址（物理地址 + 偏移量）
    let virt = physical_memory_offset + phys.as_u64();
    let page_table_ptr: *mut x86_64::structures::paging::PageTable = virt.as_mut_ptr();

    let level_4_table = &mut *page_table_ptr;
    OffsetPageTable::new(level_4_table, physical_memory_offset)
}