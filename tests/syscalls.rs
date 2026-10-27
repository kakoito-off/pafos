#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(pafos::test_runner)]
#![reexport_test_harness_main = "test_main"]

use bootloader::{BootInfo, entry_point};
use core::arch::asm;
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
    assert!((0..).map_while(process::program_at).any(|n| n.as_str() == "shell"));
    assert!(process::program_at(1000).is_none());
    assert_eq!(process::exec("no-such-program", &[]), Err(Error::NotFound));
}

#[test_case]
fn syscall_changes_only_rax() {
    let (rdi, rsi, rdx, r10, rcx, r8, r9, r11): (u64, u64, u64, u64, u64, u64, u64, u64);
    let ret: i64;
    unsafe {
        asm!(
            "int 0x80",
            inlateout("rax") pafos_abi::nr::UPTIME_MS => ret,
            inlateout("rdi") 0x1111_u64 => rdi,
            inlateout("rsi") 0x2222_u64 => rsi,
            inlateout("rdx") 0x3333_u64 => rdx,
            inlateout("r10") 0x4444_u64 => r10,
            inlateout("rcx") 0x5555_u64 => rcx,
            inlateout("r8") 0x6666_u64 => r8,
            inlateout("r9") 0x7777_u64 => r9,
            inlateout("r11") 0x8888_u64 => r11,
        );
    }
    assert!(ret >= 0);
    assert_eq!(
        [rdi, rsi, rdx, r10, rcx, r8, r9, r11],
        [0x1111, 0x2222, 0x3333, 0x4444, 0x5555, 0x6666, 0x7777, 0x8888]
    );
}

#[test_case]
fn syscall_gate_is_open_to_ring_3() {
    use x86_64::instructions::tables::sidt;
    // Each 64-bit IDT entry is 16 bytes; bits 45..46 of the first quadword
    // are the DPL, bits 40..43 the gate type (0xF = trap gate).
    let idt = sidt();
    let entry = idt.base.as_u64() + 16 * pafos_abi::SYSCALL_VECTOR as u64;
    let low = unsafe { *(entry as *const u64) };
    assert_eq!((low >> 45) & 0b11, 3, "DPL");
    assert_eq!((low >> 40) & 0xf, 0xf, "gate type");
}
