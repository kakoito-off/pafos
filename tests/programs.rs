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
use pafos_calc::{CalcError, eval};

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

#[test_case]
fn calc_follows_precedence_and_parentheses() {
    assert_eq!(eval("2+3*4"), Ok(14));
    assert_eq!(eval("(2 + 3) * 4"), Ok(20));
    assert_eq!(eval("10 - 4 - 3"), Ok(3));
    assert_eq!(eval("-7 / 2"), Ok(-3));
    assert_eq!(eval("-7 % 3"), Ok(-1));
    assert_eq!(eval("--5 + +1"), Ok(6));
    assert_eq!(eval(" 42 "), Ok(42));
}

#[test_case]
fn calc_reports_errors() {
    assert_eq!(eval(""), Err(CalcError::UnexpectedEnd));
    assert_eq!(eval("2 +"), Err(CalcError::UnexpectedEnd));
    assert_eq!(eval("(1"), Err(CalcError::UnexpectedEnd));
    assert_eq!(eval("1)"), Err(CalcError::UnexpectedChar(')')));
    assert_eq!(eval("2 x 3"), Err(CalcError::UnexpectedChar('x')));
    assert_eq!(eval("1 2"), Err(CalcError::UnexpectedChar('2')));
    assert_eq!(eval("5 / 0"), Err(CalcError::DivisionByZero));
    assert_eq!(eval("5 % (2 - 2)"), Err(CalcError::DivisionByZero));
    assert_eq!(eval("9223372036854775807 + 1"), Err(CalcError::Overflow));
    assert_eq!(eval("99999999999999999999"), Err(CalcError::Overflow));
    assert_eq!(eval("(0 - 9223372036854775807 - 1) / -1"), Err(CalcError::Overflow));
    let deep = "(".repeat(100) + "1" + &")".repeat(100);
    assert_eq!(eval(&deep), Err(CalcError::TooDeep));
}

#[test_case]
fn calc_runs_as_a_program() {
    assert_eq!(run("calc", &["2", "+", "2"]), Exit::Success);
    assert_eq!(run("calc", &["1/0"]), Exit::Error);
}

#[test_case]
fn ring3_faults_kill_only_the_program() {
    for mode in ["null", "kernel", "ud", "hlt", "panic"] {
        assert_eq!(process::exec("fault", &[mode]), Ok(Exit::Crashed), "fault {}", mode);
        // The kernel is fine and can run the next program.
        assert_eq!(run("time", &[]), Exit::Success, "after fault {}", mode);
    }
}

#[test_case]
fn ring3_pointers_to_kernel_memory_are_refused() {
    assert_eq!(run("fault", &["badptr"]), Exit::Success);
}

#[test_case]
fn ring3_programs_have_a_heap() {
    assert_eq!(run("fault", &["heap"]), Exit::Success);
}

#[test_case]
fn ring3_programs_give_back_their_memory() {
    // Warm up: the frame allocator's free list grows on first use.
    run("time", &[]);
    let before = pafos::memory::frames_in_use();
    for _ in 0..20 {
        run("time", &[]);
        process::exec("fault", &["null"]).unwrap();
    }
    assert_eq!(pafos::memory::frames_in_use(), before);
}
