//! `fault <mode>`: misbehaves on purpose, to check that the kernel isolates
//! ring 3 programs. Every crash mode must end with the program killed and the
//! system still running.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::format;
use alloc::vec::Vec;
use core::arch::asm;
use libpafos::{Error, Exit, println, process, raw};
use pafos_abi::MAX_NESTING;

libpafos::entry!(main);

/// An address inside the kernel image; mapped, but not for ring 3.
const KERNEL_ADDR: usize = 0x20_0000;

const USAGE: &str = "usage: fault (null | kernel | ud | hlt | panic | badptr | heap | nest | nestcrash)";

fn main(args: &[&str]) -> Exit {
    match args {
        ["nest"] => nest(1),
        ["nest", level] => level.parse().map_or(Exit::Error, nest),
        [mode] => crash_or_survive(mode),
        _ => {
            println!("{}", USAGE);
            Exit::Error
        }
    }
}

fn crash_or_survive(mode: &str) -> Exit {
    match mode {
        // Crashes: each one should get the program killed.
        "null" => unsafe { core::ptr::write_volatile(0x10 as *mut u8, 1) },
        "kernel" => {
            let value = unsafe { core::ptr::read_volatile(KERNEL_ADDR as *const u8) };
            println!("read kernel memory: {}", value);
        }
        "ud" => unsafe { asm!("ud2") },
        "hlt" => unsafe { asm!("hlt") },
        "panic" => panic!("on purpose"),

        // Non-crashes: things a program may do and survive.
        "badptr" => {
            // The kernel must refuse to read kernel memory on our behalf.
            let result = unsafe { raw::syscall(pafos_abi::nr::WRITE, [pafos_abi::STDOUT, KERNEL_ADDR, 16, 0]) };
            println!("write from kernel memory: {:?}", result);
            return if result.is_err() { Exit::Success } else { Exit::Error };
        }
        "heap" => {
            let numbers: Vec<u64> = (1..=1000).collect();
            let sum: u64 = numbers.iter().sum();
            println!("sum of 1..=1000 = {}", sum);
            return if sum == 500_500 { Exit::Success } else { Exit::Error };
        }
        "nestcrash" => {
            // A crashing child must not take us down with it.
            let exit = process::exec("fault", &["null"]);
            println!("child: {:?}", exit);
            return if exit == Ok(Exit::Crashed) { Exit::Success } else { Exit::Error };
        }
        _ => {
            println!("{}", USAGE);
            return Exit::Error;
        }
    }
    println!("still alive, the fault did not happen");
    Exit::Error
}

/// `fault nest <level>`: starts `fault nest <level + 1>`, and so on, until the
/// kernel refuses to go deeper. That must happen exactly at `MAX_NESTING`
/// (the caller of `fault nest` counts as level 0). Every level checks that
/// its own memory survived the deeper ones.
fn nest(level: usize) -> Exit {
    let mine: Vec<usize> = (0..100).map(|i| i * level).collect();
    let next = format!("{}", level + 1);
    let exit = match process::exec("fault", &["nest", &next]) {
        Ok(exit) => exit,
        Err(Error::TooDeep) if level == MAX_NESTING => Exit::Success,
        Err(err) => {
            println!("fault nest {}: {}", level, err);
            Exit::Error
        }
    };
    let intact = mine.iter().enumerate().all(|(i, &v)| v == i * level);
    if intact { exit } else { Exit::Error }
}
