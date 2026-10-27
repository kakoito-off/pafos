#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(pafos::test_runner)]
#![reexport_test_harness_main = "test_main"]

use bootloader::{BootInfo, entry_point};
use core::panic::PanicInfo;
use libpafos::{Error, process, raw};

entry_point!(main);

fn main(boot_info: &'static BootInfo) -> ! {
    pafos::init_core();
    pafos::init_memory(boot_info);
    test_main();
    loop {}
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    pafos::test_panic_handler(info)
}

#[test_case]
fn unknown_syscall_is_an_error() {
    assert_eq!(unsafe { raw::syscall(9999, [0; 4]) }, Err(Error::NoSyscall));
}

#[test_case]
fn bad_file_descriptors_are_rejected() {
    let buf = [0u8; 4];
    let write = |fd| unsafe { raw::syscall(pafos_abi::nr::WRITE, [fd, buf.as_ptr() as usize, 4, 0]) };
    assert_eq!(write(0), Err(Error::BadFd));
    assert_eq!(write(1000), Err(Error::BadFd));
    assert_eq!(write(pafos_abi::STDOUT), Ok(4));
}

#[test_case]
fn null_pointers_are_rejected() {
    assert_eq!(unsafe { raw::syscall(pafos_abi::nr::CREATE, [0, 3, 0, 0]) }, Err(Error::InvalidArgument));
}

#[test_case]
fn programs_are_listed_and_unknown_ones_not_found() {
    assert_eq!(process::program_at(0).map(|n| n.as_str() == "shell"), Some(true));
    assert!(process::program_at(1000).is_none());
    assert_eq!(process::exec("no-such-program", &[]), Err(Error::NotFound));
}
