//! `time`: how long the system has been running.

#![no_std]
#![no_main]

use libpafos::time::{self, Elapsed};
use libpafos::{Exit, println};

libpafos::entry!(main);

fn main(args: &[&str]) -> Exit {
    if !args.is_empty() {
        println!("usage: time");
        return Exit::Error;
    }
    println!("pafos running {}", Elapsed(time::uptime_ms()));
    Exit::Success
}
