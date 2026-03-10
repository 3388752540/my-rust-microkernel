use xmas_elf::{ElfFile, program::{Type, ProgramHeader}};
use x86_64::{VirtAddr, structures::paging::{Page, PageTableFlags, Size4KiB, Mapper, FrameAllocator}};

pub unsafe fn load_elf(
    elf_data: &[u8],
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>
) -> VirtAddr {
    let elf = ElfFile::new(elf_data).expect("Failed to parse ELF file");
    
    for header in elf.program_iter() {
        if let Type::Load = header.get_type().unwrap() {
            unsafe { load_segment(elf_data, &header, mapper, frame_allocator) };
        }
    }

    VirtAddr::new(elf.header.pt2.entry_point())
}

unsafe fn load_segment(
    elf_data: &[u8],
    header: &ProgramHeader,
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>
) {
    let virt_start_addr = VirtAddr::new(header.virtual_addr());
    let mem_size = header.mem_size();
    let file_size = header.file_size();
    let file_offset = header.offset() as usize;

    let start_page = Page::<Size4KiB>::containing_address(virt_start_addr);
    let end_page = Page::<Size4KiB>::containing_address(virt_start_addr + mem_size - 1u64);

    let mut flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
    if header.flags().is_write() {
        flags |= PageTableFlags::WRITABLE;
    }

    for page in Page::range_inclusive(start_page, end_page) {
        let frame = frame_allocator.allocate_frame().expect("Out of physical memory");
        // 先以可写模式映射，方便拷贝
        unsafe { mapper.map_to(page, frame, flags | PageTableFlags::WRITABLE, frame_allocator)
            .expect("Failed to map segment")
            .flush() };
    }

    // 拷贝数据并处理 BSS
    let dest_ptr = virt_start_addr.as_mut_ptr::<u8>();
    unsafe { core::ptr::write_bytes(dest_ptr, 0, mem_size as usize) };
    unsafe { core::ptr::copy_nonoverlapping(
        elf_data.as_ptr().add(file_offset),
        dest_ptr,
        file_size as usize
    ) };

    // 权限修正 (W^X)
    if !header.flags().is_write() {
        for page in Page::range_inclusive(start_page, end_page) {
            unsafe { mapper.update_flags(page, flags).expect("Failed to update flags").flush() };
        }
    }
}