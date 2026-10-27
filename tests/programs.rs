#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(pafos::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use alloc::format;
use bootloader::{BootInfo, entry_point};
use core::panic::PanicInfo;
use libpafos::fs::{self, File, Mode};
use libpafos::time::{Elapsed, uptime_ms};
use libpafos::{Error, Exit, process};

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

fn run(name: &str, args: &[&str]) -> Exit {
    process::exec(name, args).unwrap()
}

#[test_case]
fn uptime_advances_with_timer_interrupts() {
    let start = uptime_ms();
    // 30 interrupts at ~100 Hz: well past the 10 ms resolution.
    for _ in 0..30 {
        x86_64::instructions::hlt();
    }
    let elapsed = uptime_ms() - start;
    assert!(elapsed >= 100 && elapsed < 2000, "elapsed {} ms", elapsed);
}

#[test_case]
fn elapsed_is_written_in_words() {
    assert_eq!(format!("{}", Elapsed(0)), "0 seconds");
    assert_eq!(format!("{}", Elapsed(1_999)), "1 second");
    assert_eq!(format!("{}", Elapsed(14_000)), "14 seconds");
    assert_eq!(format!("{}", Elapsed(60_000)), "1 minute");
    assert_eq!(format!("{}", Elapsed(63_000)), "1 minute 3 seconds");
    assert_eq!(format!("{}", Elapsed(7_201_000)), "2 hours 1 second");
}

#[test_case]
fn time_rejects_arguments() {
    assert_eq!(run("time", &[]), Exit::Success);
    assert_eq!(run("time", &["now"]), Exit::Error);
}

#[test_case]
fn timer_keeps_one_start_mark_in_its_file() {
    assert_eq!(run("timer", &[]), Exit::Error);
    assert_eq!(run("timer", &["show"]), Exit::Success); // "not started"

    assert_eq!(run("timer", &["start"]), Exit::Success);
    let first = fs::read_to_string("timer").unwrap();
    assert!(first.starts_with("start "));
    assert_eq!(run("timer", &["show"]), Exit::Success);

    for _ in 0..5 {
        x86_64::instructions::hlt();
    }
    assert_eq!(run("timer", &["start"]), Exit::Success);
    let second = fs::read_to_string("timer").unwrap();
    assert_eq!(second.lines().count(), 1, "restart overwrites the mark");
    assert_ne!(first, second);

    assert_eq!(run("timer", &["clear"]), Exit::Success);
    assert_eq!(File::open("timer", Mode::Read).err(), Some(Error::NotFound));
    assert_eq!(run("timer", &["clear"]), Exit::Success);
}

#[test_case]
fn timer_reports_a_damaged_file() {
    fs::create("timer").unwrap();
    File::open("timer", Mode::Write).unwrap().write(b"garbage").unwrap();
    assert_eq!(run("timer", &["show"]), Exit::Error);
    assert_eq!(run("timer", &["clear"]), Exit::Success);
}
