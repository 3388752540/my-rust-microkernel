use x86_64::{
    structures::paging::{
        mapper::MapToError, FrameAllocator, Mapper, Page, PageTableFlags, Size4KiB,
    },
    VirtAddr,
};
use linked_list_allocator::LockedHeap;

// 1. 定义堆的起始地址和大小
// 我们选择一个比较高的虚拟地址，避免与内核代码冲突
pub const HEAP_START: usize = 0x_4444_4444_0000;
pub const HEAP_SIZE: usize = 100 * 1024; // 暂时先分配 100 KiB 堆空间

// 2. 告诉 Rust 编译器，这是全局动态内存分配器
#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

/// 初始化内核堆
/// mapper: 负责修改页表的工具
/// frame_allocator: 负责提供物理页帧的工具
pub fn init_heap(
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) -> Result<(), MapToError<Size4KiB>> {
    // 定义我们要映射的虚拟地址范围
    let page_range = {
        let heap_start = VirtAddr::new(HEAP_START as u64);
        let heap_end = heap_start + HEAP_SIZE - 1u64;
        let heap_start_page = Page::containing_address(heap_start);
        let heap_end_page = Page::containing_address(heap_end);
        Page::range_inclusive(heap_start_page, heap_end_page)
    };

    // 遍历每一页，将其映射到物理页帧
    for page in page_range {
        let frame = frame_allocator
            .allocate_frame()
            .ok_or(MapToError::FrameAllocationFailed)?;
        
        // 设置页表标志：必须存在，且可读写
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        
        // 这一步是核心：修改页表条目，建立映射
        unsafe {
            mapper.map_to(page, frame, flags, frame_allocator)?.flush();
        }
    }

    // 映射完成后，告诉分配器：你可以开始从这段空间分发内存了
    unsafe {
        ALLOCATOR.lock().init(HEAP_START as *mut u8, HEAP_SIZE);
    }

    Ok(())
}