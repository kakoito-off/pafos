#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(pafos::test_runner)]
#![reexport_test_harness_main = "test_main"]

use bootloader::{BootInfo, entry_point};
use core::panic::PanicInfo;
use pafos::{println, programs, vga_buffer};

entry_point!(kernel_main);

fn kernel_main(boot_info: &'static BootInfo) -> ! {
    pafos::init_core();
    pafos::init_memory(boot_info);

    #[cfg(test)]
    test_main();

    loop {
        let exit = match programs::run("shell", &[]) {
            Ok(exit) => exit,
            // No shell in this build: nothing to run, idle on a blank screen.
            Err(_) => {
                vga_buffer::clear_screen();
                pafos::hlt_loop();
            }
        };
        vga_buffer::clear_screen();
        println!("shell exited ({:?}), starting a new one", exit);
    }
}

/// This function is called on panic.
#[cfg(not(test))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);
    pafos::hlt_loop();
}

#[cfg(test)]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    pafos::test_panic_handler(info)
}

#[test_case]
fn trivial_assertion() {
    assert_eq!(1, 1);
}
