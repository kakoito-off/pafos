#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(pafos::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use alloc::vec::Vec;
use bootloader::{BootInfo, entry_point};
use core::panic::PanicInfo;
use pafos::sys::{self, File, FsError, Mode};

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

fn read_all(name: &str) -> Vec<u8> {
    let mut file = File::open(name, Mode::Read).unwrap();
    let mut out = Vec::new();
    let mut buf = [0u8; 100];
    loop {
        let n = file.read(&mut buf).unwrap();
        if n == 0 {
            return out;
        }
        out.extend_from_slice(&buf[..n]);
    }
}

fn free_blocks() -> usize {
    sys::fs_stats().unwrap().free_blocks
}

#[test_case]
fn write_then_read() {
    File::open("a", Mode::Write).unwrap().write(b"hello").unwrap();
    assert_eq!(read_all("a"), b"hello");
    sys::remove_file("a").unwrap();
}

#[test_case]
fn spans_multiple_blocks_and_appends() {
    let first: Vec<u8> = (0..1300u32).map(|i| (i % 251) as u8).collect();
    File::open("big", Mode::Write).unwrap().write(&first).unwrap();
    File::open("big", Mode::Append).unwrap().write(b"tail").unwrap();

    let data = read_all("big");
    assert_eq!(data.len(), 1304);
    assert_eq!(&data[..1300], &first[..]);
    assert_eq!(&data[1300..], b"tail");
    sys::remove_file("big").unwrap();
}

#[test_case]
fn write_mode_truncates() {
    File::open("t", Mode::Write).unwrap().write(b"long old content").unwrap();
    File::open("t", Mode::Write).unwrap().write(b"new").unwrap();
    assert_eq!(read_all("t"), b"new");
    sys::remove_file("t").unwrap();
}

#[test_case]
fn remove_releases_blocks() {
    let before = free_blocks();
    File::open("r", Mode::Write).unwrap().write(&[7u8; 2000]).unwrap();
    assert_eq!(free_blocks(), before - 4);
    sys::remove_file("r").unwrap();
    assert_eq!(free_blocks(), before);
}

#[test_case]
fn reports_errors() {
    assert_eq!(File::open("missing", Mode::Read).err(), Some(FsError::NotFound));
    assert_eq!(File::open("", Mode::Write).err(), Some(FsError::InvalidName));
    assert_eq!(File::open("a/b", Mode::Write).err(), Some(FsError::InvalidName));

    let mut file = File::open("e", Mode::Write).unwrap();
    assert_eq!(sys::remove_file("e"), Err(FsError::Busy));
    assert_eq!(File::open("e", Mode::Read).err(), Some(FsError::Busy));
    assert_eq!(file.read(&mut [0u8; 4]), Err(FsError::WrongMode));
    drop(file);
    sys::remove_file("e").unwrap();
}

#[test_case]
fn full_disk_is_an_error_not_a_panic() {
    let before = free_blocks();
    let chunk = [1u8; 4096];
    let mut file = File::open("fill", Mode::Write).unwrap();
    let err = loop {
        if let Err(err) = file.write(&chunk) {
            break err;
        }
    };
    assert_eq!(err, FsError::NoSpace);
    assert_eq!(free_blocks(), 0);
    drop(file);
    sys::remove_file("fill").unwrap();
    assert_eq!(free_blocks(), before);
}
