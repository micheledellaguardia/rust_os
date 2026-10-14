//! Paging, physical frame allocation and the kernel heap.

use bootloader_api::info::{MemoryRegion, MemoryRegionKind};
use linked_list_allocator::LockedHeap;
use x86_64::structures::paging::mapper::MapToError;
use x86_64::structures::paging::{
    FrameAllocator, Mapper, OffsetPageTable, Page, PageTable, PageTableFlags, PhysFrame, Size4KiB,
};
use x86_64::{PhysAddr, VirtAddr};

/// Virtual start address of the kernel heap.
pub const HEAP_START: u64 = 0x_4444_4444_0000;
/// Size of the kernel heap in bytes.
pub const HEAP_SIZE: usize = 4 * 1024 * 1024; // 4 MiB

/// The global allocator backing all `alloc` allocations in the kernel.
#[global_allocator]
pub static ALLOCATOR: LockedHeap = LockedHeap::empty();

/// Creates a new `OffsetPageTable` for the currently active level-4 table.
///
/// # Safety
/// The caller must guarantee that the complete physical memory is mapped to the
/// virtual address space starting at `physical_memory_offset`, and that this
/// function is only called once.
pub unsafe fn init(physical_memory_offset: VirtAddr) -> OffsetPageTable<'static> {
    let level_4_table = unsafe { active_level_4_table(physical_memory_offset) };
    OffsetPageTable::new(level_4_table, physical_memory_offset)
}

unsafe fn active_level_4_table(physical_memory_offset: VirtAddr) -> &'static mut PageTable {
    use x86_64::registers::control::Cr3;

    let (level_4_table_frame, _) = Cr3::read();
    let physical = level_4_table_frame.start_address();
    let virtual_address = physical_memory_offset + physical.as_u64();
    let page_table_ptr: *mut PageTable = virtual_address.as_mut_ptr();

    unsafe { &mut *page_table_ptr }
}

/// Translates the given virtual address to the mapped physical address using the active level 4 table.
pub fn translate_addr(virt: VirtAddr, physical_memory_offset: VirtAddr) -> Option<PhysAddr> {
    use x86_64::structures::paging::Translate;
    let mapper = unsafe { init(physical_memory_offset) };
    mapper.translate_addr(virt)
}

/// A `FrameAllocator` that hands out frames from the bootloader memory map.
pub struct BootInfoFrameAllocator {
    memory_regions: &'static mut [MemoryRegion],
    next: usize,
}

impl BootInfoFrameAllocator {
    /// # Safety
    /// The caller must guarantee that no other code allocates these frames.
    pub unsafe fn init(memory_regions: &'static mut [MemoryRegion]) -> Self {
        BootInfoFrameAllocator {
            memory_regions,
            next: 0,
        }
    }

    fn usable_frames(&self) -> impl Iterator<Item = PhysFrame> + '_ {
        self.memory_regions
            .iter()
            .filter(|region| region.kind == MemoryRegionKind::Usable)
            .map(|region| region.start..region.end)
            .map(|range| {
                PhysFrame::range(
                    PhysFrame::containing_address(PhysAddr::new(range.start)),
                    PhysFrame::containing_address(PhysAddr::new(range.end - 1)),
                )
            })
            .flat_map(|frames| frames)
    }
}

unsafe impl FrameAllocator<Size4KiB> for BootInfoFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        let frame = self.usable_frames().nth(self.next);
        self.next += 1;
        frame
    }
}

/// Maps the kernel heap and hands it to the global allocator.
pub fn init_heap(
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) -> Result<(), MapToError<Size4KiB>> {
    let heap_start = VirtAddr::new(HEAP_START);
    let heap_end = heap_start + HEAP_SIZE as u64 - 1;

    for page in Page::range_inclusive(
        Page::containing_address(heap_start),
        Page::containing_address(heap_end),
    ) {
        let frame = frame_allocator
            .allocate_frame()
            .ok_or(MapToError::FrameAllocationFailed)?;
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        unsafe {
            mapper
                .map_to(page, frame, flags, frame_allocator)?
                .flush();
        }
    }

    unsafe {
        ALLOCATOR.lock().init(heap_start.as_mut_ptr(), HEAP_SIZE);
    }

    Ok(())
}

// Maps a range of physical memory (e.g. device MMIO BAR) with caching disabled.
/* pub fn map_mmio_range(
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
    phys_start: PhysAddr,
    virt_start: VirtAddr,
    size_bytes: usize,
) -> Result<(), MapToError<Size4KiB>> {
    let start_page = Page::<Size4KiB>::containing_address(virt_start);
    let end_page = Page::<Size4KiB>::containing_address(virt_start + (size_bytes as u64 - 1));

    let start_frame = PhysFrame::<Size4KiB>::containing_address(phys_start);
    let mut current_frame = start_frame;

    let flags = PageTableFlags::PRESENT
        | PageTableFlags::WRITABLE
        | PageTableFlags::NO_CACHE
        | PageTableFlags::WRITE_THROUGH;

    for page in Page::range_inclusive(start_page, end_page) {
        unsafe {
            mapper
                .map_to(page, current_frame, flags, frame_allocator)?
                .flush();
        }
        current_frame = PhysFrame::containing_address(current_frame.start_address() + 4096u64);
    }

    Ok(())
} */

