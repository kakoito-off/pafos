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

/// Creates `name` and opens it for writing.
fn new_file(name: &str) -> File {
    sys::create_file(name).unwrap();
    File::open(name, Mode::Write).unwrap()
}

fn free_blocks() -> usize {
    sys::fs_stats().unwrap().free_blocks
}

#[test_case]
fn write_then_read() {
    new_file("a").write(b"hello").unwrap();
    assert_eq!(read_all("a"), b"hello");
    sys::remove_file("a").unwrap();
}

#[test_case]
fn spans_multiple_blocks_and_appends() {
    let first: Vec<u8> = (0..1300u32).map(|i| (i % 251) as u8).collect();
    new_file("big").write(&first).unwrap();
    File::open("big", Mode::Append).unwrap().write(b"tail").unwrap();

    let data = read_all("big");
    assert_eq!(data.len(), 1304);
    assert_eq!(&data[..1300], &first[..]);
    assert_eq!(&data[1300..], b"tail");
    sys::remove_file("big").unwrap();
}

#[test_case]
fn write_mode_truncates() {
    new_file("t").write(b"long old content").unwrap();
    File::open("t", Mode::Write).unwrap().write(b"new").unwrap();
    assert_eq!(read_all("t"), b"new");
    sys::remove_file("t").unwrap();
}

#[test_case]
fn remove_releases_blocks() {
    let before = free_blocks();
    new_file("r").write(&[7u8; 2000]).unwrap();
    assert_eq!(free_blocks(), before - 4);
    sys::remove_file("r").unwrap();
    assert_eq!(free_blocks(), before);
}

#[test_case]
fn reports_errors() {
    assert_eq!(File::open("missing", Mode::Read).err(), Some(FsError::NotFound));
    assert_eq!(sys::create_file(""), Err(FsError::InvalidName));
    assert_eq!(sys::create_file("a/b"), Err(FsError::InvalidName));

    let mut file = new_file("e");
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
    let mut file = new_file("fill");
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

#[test_case]
fn open_never_creates_files() {
    assert_eq!(File::open("nope", Mode::Write).err(), Some(FsError::NotFound));
    assert_eq!(File::open("nope", Mode::Append).err(), Some(FsError::NotFound));
    assert!(sys::file_at(0).is_none());
}

#[test_case]
fn create_makes_empty_file_once() {
    sys::create_file("c").unwrap();
    assert_eq!(read_all("c"), b"");
    assert_eq!(sys::create_file("c"), Err(FsError::AlreadyExists));
    sys::remove_file("c").unwrap();
}

#[test_case]
fn append_adds_to_existing_content() {
    new_file("lines").write(b"one\n").unwrap();
    File::open("lines", Mode::Append).unwrap().write(b"two\n").unwrap();
    assert_eq!(read_all("lines"), b"one\ntwo\n");
    // Opening for write with nothing written is how a file gets cleared.
    drop(File::open("lines", Mode::Write).unwrap());
    assert_eq!(read_all("lines"), b"");
    sys::remove_file("lines").unwrap();
}
