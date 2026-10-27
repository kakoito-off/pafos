//! `timer`: a stopwatch whose start mark is kept in the file `timer`, as a
//! line `start <uptime in ms>`, so it can be inspected with `cat timer`.

#![no_std]

extern crate alloc;

use alloc::format;
use libpafos::fs::{self, File, Mode};
use libpafos::time::{self, Elapsed};
use libpafos::{Error, Exit, println};

const FILE: &str = "timer";

pub fn main(args: &[&str]) -> Exit {
    let result = match args {
        ["start"] => start(),
        ["show"] => show(),
        ["clear"] => clear(),
        _ => {
            println!("usage: timer (start | show | clear)");
            return Exit::Error;
        }
    };
    match result {
        Ok(()) => Exit::Success,
        Err(Error::InvalidArgument) => {
            println!("timer: file `{}` is damaged, run `timer clear`", FILE);
            Exit::Error
        }
        Err(err) => {
            println!("timer: {}", err);
            Exit::Error
        }
    }
}

/// Starts the timer, overwriting a previous start mark.
fn start() -> Result<(), Error> {
    match fs::create(FILE) {
        Ok(()) | Err(Error::AlreadyExists) => {}
        Err(err) => return Err(err),
    }
    let mark = format!("start {}\n", time::uptime_ms());
    File::open(FILE, Mode::Write)?.write(mark.as_bytes())?;
    println!("timer started");
    Ok(())
}

fn show() -> Result<(), Error> {
    let Some(started) = read_mark()? else {
        println!("timer is not started");
        return Ok(());
    };
    let elapsed = time::uptime_ms().saturating_sub(started);
    println!("timer now at {}", Elapsed(elapsed));
    Ok(())
}

fn clear() -> Result<(), Error> {
    match fs::remove(FILE) {
        Ok(()) | Err(Error::NotFound) => {}
        Err(err) => return Err(err),
    }
    println!("timer cleared");
    Ok(())
}

/// The start mark, or `None` if the timer is not started;
/// `InvalidArgument` if the file holds something else.
fn read_mark() -> Result<Option<u64>, Error> {
    let text = match fs::read_to_string(FILE) {
        Ok(text) => text,
        Err(Error::NotFound) => return Ok(None),
        Err(err) => return Err(err),
    };
    text.trim()
        .strip_prefix("start ")
        .and_then(|ms| ms.parse().ok())
        .map(Some)
        .ok_or(Error::InvalidArgument)
}
