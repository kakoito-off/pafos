//! `fault <mode>`: misbehaves on purpose, to check that the kernel isolates
//! ring 3 programs. Every crash mode must end with the program killed and the
//! system still running.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;
use core::arch::asm;
use libpafos::{Exit, println, raw};

libpafos::entry!(main);

/// An address inside the kernel image; mapped, but not for ring 3.
const KERNEL_ADDR: usize = 0x20_0000;

const USAGE: &str = "usage: fault (null | kernel | ud | hlt | panic | badptr | heap)";

fn main(args: &[&str]) -> Exit {
    let [mode] = args else {
        println!("{}", USAGE);
        return Exit::Error;
    };
    match *mode {
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
        _ => {
            println!("{}", USAGE);
            return Exit::Error;
        }
    }
    println!("still alive, the fault did not happen");
    Exit::Error
}
