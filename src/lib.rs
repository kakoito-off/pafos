#![no_std]
#![cfg_attr(test, no_main)]
#![feature(custom_test_frameworks)]
#![feature(abi_x86_interrupt)]
#![test_runner(crate::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;
use bootloader::BootInfo;
use core::panic::PanicInfo;

pub mod allocator;
pub mod gdt;
mod input;
pub mod interrupts;
pub mod memory;
pub mod programs;
pub mod serial;
pub mod sys;
mod tmpfs;
pub mod vga_buffer;

/// Sets up everything the CPU needs before it can safely run kernel code:
/// GDT/TSS, IDT, the PIC chips, and finally enables interrupts.
pub fn init_core() {
    gdt::init();
    interrupts::init_idt();
    unsafe { interrupts::PICS.lock().initialize() };
    x86_64::instructions::interrupts::enable();
}

/// Sets up paging, the heap (so `Box`, `Vec`, `String`, ... work) and the
/// tmpfs data region. Must run after `init_core()` (page faults need a working
/// IDT) and needs the `BootInfo` passed in by the bootloader.
pub fn init_memory(boot_info: &'static BootInfo) {
    use x86_64::VirtAddr;
    use x86_64::structures::paging::PageTableFlags;

    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset);
    let mut mapper = unsafe { memory::init(phys_mem_offset) };
    let mut frame_allocator = unsafe { memory::BootInfoFrameAllocator::init(&boot_info.memory_map) };

    allocator::init_heap(&mut mapper, &mut frame_allocator).expect("heap initialization failed");

    memory::map_region(
        &mut mapper,
        &mut frame_allocator,
        VirtAddr::new(tmpfs::TMPFS_START as u64),
        tmpfs::TMPFS_SIZE as u64,
        PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
    )
    .expect("tmpfs region mapping failed");
    // The region was just mapped and nothing else uses it; the heap is ready.
    unsafe { tmpfs::init(tmpfs::TMPFS_START, tmpfs::TMPFS_SIZE) };
}
pub trait Testable {
    fn run(&self) -> ();
}

impl<T> Testable for T
where
    T: Fn(),
{
    fn run(&self) {
        serial_print!("{}...\t", core::any::type_name::<T>());
        self();
        serial_println!("[ok]");
    }
}

pub fn test_runner(tests: &[&dyn Testable]) {
    serial_println!("Running {} tests", tests.len());
    for test in tests {
        test.run();
    }
    exit_qemu(QemuExitCode::Success);
}

pub fn test_panic_handler(info: &PanicInfo) -> ! {
    serial_println!("[failed]\n");
    serial_println!("Error: {}\n", info);
    exit_qemu(QemuExitCode::Failed);
    hlt_loop();
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum QemuExitCode {
    Success = 0x10,
    Failed = 0x11,
}

pub fn exit_qemu(exit_code: QemuExitCode) {
    use x86_64::instructions::port::Port;

    unsafe {
        let mut port = Port::new(0xf4);
        port.write(exit_code as u32);
    }
}

pub fn hlt_loop() -> ! {
    loop {
        x86_64::instructions::hlt();
    }
}

#[cfg(test)]
use bootloader::entry_point;

#[cfg(test)]
entry_point!(test_kernel_main);

/// Entry point for `cargo xtest`
#[cfg(test)]
fn test_kernel_main(boot_info: &'static BootInfo) -> ! {
    init_core();
    init_memory(boot_info);
    test_main();
    hlt_loop();
}

#[cfg(test)]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    test_panic_handler(info)
}
