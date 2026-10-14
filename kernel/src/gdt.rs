//! Global Descriptor Table + Task State Segment setup.
//!
//! The TSS provides a dedicated (known-good) stack for the double-fault handler
//! via an IST entry, so a stack overflow is reported as a double fault instead
//! of triggering a triple fault reset.

use spin::Once;
use x86_64::instructions::tables::load_tss;
use x86_64::registers::segmentation::{Segment, CS, DS, ES, SS};
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;
use x86_64::VirtAddr;

pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;

struct Selectors {
    code_selector: SegmentSelector,
    data_selector: SegmentSelector,
    tss_selector: SegmentSelector,
}

static TSS: Once<TaskStateSegment> = Once::new();
static GDT: Once<(GlobalDescriptorTable, Selectors)> = Once::new();

fn tss() -> &'static TaskStateSegment {
    TSS.call_once(|| unsafe {
        const STACK_SIZE: usize = 4096 * 5;
        static mut DOUBLE_FAULT_STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];

        let mut tss = TaskStateSegment::new();
        let stack_start = VirtAddr::from_ptr(&raw const DOUBLE_FAULT_STACK);
        let stack_end = stack_start + STACK_SIZE as u64;
        tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = stack_end;
        tss
    })
}

pub fn init() {
    let tss = tss();

    let (gdt, selectors) = GDT.call_once(|| {
        let mut gdt = GlobalDescriptorTable::new();
        let code_selector = gdt.append(Descriptor::kernel_code_segment());
        let data_selector = gdt.append(Descriptor::kernel_data_segment());
        let tss_selector = gdt.append(Descriptor::tss_segment(tss));
        (
            gdt,
            Selectors {
                code_selector,
                data_selector,
                tss_selector,
            },
        )
    });

    gdt.load();

    unsafe {
        // Reload the segment registers so they point at our descriptors. `SS`
        // in particular must be a valid data segment, otherwise `iretq` from an
        // interrupt handler would fault.
        CS::set_reg(selectors.code_selector);
        SS::set_reg(selectors.data_selector);
        DS::set_reg(selectors.data_selector);
        ES::set_reg(selectors.data_selector);
        load_tss(selectors.tss_selector);
    }
}
