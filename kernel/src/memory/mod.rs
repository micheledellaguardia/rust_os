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
pub const HEAP_SIZE: usize = 1024 * 1024; // 1 MiB

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

/// Returns a mutable reference to the currently active level-4 page table.
///
/// # Safety
/// See [`init`].
unsafe fn active_level_4_table(physical_memory_offset: VirtAddr) -> &'static mut PageTable {
    use x86_64::registers::control::Cr3;

    let (level_4_table_frame, _) = Cr3::read();
    let physical = level_4_table_frame.start_address();
    let virtual_address = physical_memory_offset + physical.as_u64();
    let page_table_ptr: *mut PageTable = virtual_address.as_mut_ptr();

    unsafe { &mut *page_table_ptr }
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
