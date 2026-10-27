//! `time`: how long the system has been running.

#![no_std]

use libpafos::time::{self, Elapsed};
use libpafos::{Exit, println};

pub fn main(args: &[&str]) -> Exit {
    if !args.is_empty() {
        println!("usage: time");
        return Exit::Error;
    }
    println!("pafos running {}", Elapsed(time::uptime_ms()));
    Exit::Success
}
