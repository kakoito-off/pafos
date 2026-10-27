#![no_std]
#![no_main]

use libpafos::fs::{File, Mode};
use libpafos::{Exit, println};

libpafos::entry!(main);

fn main(args: &[&str]) -> Exit {
    let [name] = args else {
        println!("usage: zeros <file>");
        return Exit::Error;
    };

    let mut file = match File::open(name, Mode::Read) {
        Ok(file) => file,
        Err(err) => {
            println!("zeros: {}: {}", name, err);
            return Exit::Error;
        }
    };

    let mut buf = [0u8; 512];
    let mut zeros = 0;
    loop {
        let n = match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(err) => {
                println!("zeros: {}: {}", name, err);
                return Exit::Error;
            }
        };
        zeros += buf[..n].iter().filter(|&&b| b == b'0').count();
    }

    println!("{}", zeros);
    Exit::Success
}