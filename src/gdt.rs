use lazy_static::lazy_static;
use x86_64::VirtAddr;
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;

pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;

/// Mutable because RSP0 changes every time a ring 3 program is entered (see
/// [`rsp0_slot`]). The CPU reads it from memory on each ring 3 → ring 0 switch.
static mut TSS: TaskStateSegment = TaskStateSegment::new();

lazy_static! {
    static ref GDT: (GlobalDescriptorTable, Selectors) = {
        let mut gdt = GlobalDescriptorTable::new();
        let code_selector = gdt.append(Descriptor::kernel_code_segment());
        let data_selector = gdt.append(Descriptor::kernel_data_segment());
        // Selectors carry the descriptor's privilege level (RPL 3) already.
        let user_data_selector = gdt.append(Descriptor::user_data_segment());
        let user_code_selector = gdt.append(Descriptor::user_code_segment());
        // The TSS is a static, so it outlives the GDT.
        let tss_selector = gdt.append(unsafe { Descriptor::tss_segment_unchecked(&raw const TSS) });
        (
            gdt,
            Selectors {
                code_selector,
                data_selector,
                user_code_selector,
                user_data_selector,
                tss_selector,
            },
        )
    };
}

struct Selectors {
    code_selector: SegmentSelector,
    data_selector: SegmentSelector,
    user_code_selector: SegmentSelector,
    user_data_selector: SegmentSelector,
    tss_selector: SegmentSelector,
}

/// Code and data segment selectors for entering ring 3 with `iretq`.
pub fn user_selectors() -> (SegmentSelector, SegmentSelector) {
    (GDT.1.user_code_selector, GDT.1.user_data_selector)
}

/// Where the TSS keeps RSP0: the stack the CPU switches to on an interrupt
/// or system call from ring 3. There is no separate stack for that: right
/// before entering a program the kernel stores its own current stack pointer
/// here, so the program's traps land just below the kernel frames that are
/// waiting for it. Nested programs thereby stack up on the one kernel stack.
pub fn rsp0_slot() -> *mut u64 {
    // `TaskStateSegment` is packed: the slot is not 8-byte aligned, which is
    // fine for the CPU and for `read/write_unaligned`.
    unsafe { (&raw mut TSS.privilege_stack_table[0]).cast() }
}

pub fn init() {
    use x86_64::instructions::segmentation::{CS, DS, ES, SS, Segment};
    use x86_64::instructions::tables::load_tss;

    unsafe {
        TSS.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = {
            const STACK_SIZE: usize = 4096 * 5;
            static mut STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];

            let stack_start = VirtAddr::from_ptr(&raw const STACK);
            stack_start + STACK_SIZE as u64
        };
    }

    GDT.0.load();
    unsafe {
        CS::set_reg(GDT.1.code_selector);
        // Selectors left over from the bootloader's GDT would point at
        // arbitrary entries of ours; `iretq` re-checks SS.
        SS::set_reg(GDT.1.data_selector);
        DS::set_reg(GDT.1.data_selector);
        ES::set_reg(GDT.1.data_selector);
        load_tss(GDT.1.tss_selector);
    }
}
