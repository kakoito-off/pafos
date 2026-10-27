//! System call dispatcher: the only way programs reach the kernel.
//!
//! Programs call `libpafos::raw::syscall`, which today calls
//! [`pafos_syscall`] directly (everything runs in ring 0). With ring 3 an
//! `int 0x80` handler will pass the saved registers to [`dispatch`] instead,
//! and the `user_*` helpers will start checking that pointers lie in the
//! program's memory. The calls themselves stay the same.

use crate::{clock, input, programs, tmpfs, vga_buffer};
use alloc::vec::Vec;
use pafos_abi::{
    Error, FIRST_FILE_FD, FsStats, Mode, READ_KEY_NONBLOCK, STDOUT, StrRef, console, encode, nr,
};
use x86_64::instructions::interrupts;

#[unsafe(no_mangle)]
pub extern "C" fn pafos_syscall(nr: usize, a0: usize, a1: usize, a2: usize, a3: usize) -> isize {
    encode(dispatch(nr, [a0, a1, a2, a3]))
}

pub fn dispatch(nr: usize, a: [usize; 4]) -> Result<usize, Error> {
    match nr {
        nr::READ_KEY => read_key(a[0]),
        nr::STOP_REQUESTED => Ok(input::stop_requested() as usize),
        nr::CLEAR_STOP => {
            input::clear_stop();
            Ok(0)
        }
        nr::CONSOLE_CTL => console_ctl(a[0]),

        nr::CREATE => tmpfs::create(user_str(a[0], a[1])?).map(|()| 0),
        nr::OPEN => {
            let fd = tmpfs::open(user_str(a[0], a[1])?, Mode::from_raw(a[2])?)?;
            Ok(fd + FIRST_FILE_FD)
        }
        nr::READ => tmpfs::read(file_fd(a[0])?, user_buf_mut(a[1], a[2])?),
        nr::WRITE => write(a[0], user_buf(a[1], a[2])?),
        nr::CLOSE => tmpfs::close(file_fd(a[0])?).map(|()| 0),
        nr::UNLINK => tmpfs::remove(user_str(a[0], a[1])?).map(|()| 0),
        nr::READDIR => fill(a[1], tmpfs::file_at(a[0])),
        nr::FSSTAT => {
            *user_mut::<FsStats>(a[0])? = tmpfs::stats()?;
            Ok(0)
        }

        nr::UPTIME_MS => Ok(clock::uptime_ms() as usize),

        nr::EXEC => exec(a),
        nr::PROGRAM_AT => fill(a[1], programs::name_at(a[0])),
        _ => Err(Error::NoSyscall),
    }
}

fn read_key(flags: usize) -> Result<usize, Error> {
    let key = if flags & READ_KEY_NONBLOCK != 0 {
        input::try_pop().ok_or(Error::WouldBlock)?
    } else {
        input::pop_blocking()
    };
    Ok(key as usize)
}

fn console_ctl(op: usize) -> Result<usize, Error> {
    match op {
        console::BACKSPACE => interrupts::without_interrupts(|| vga_buffer::WRITER.lock().backspace()),
        console::CLEAR_SCREEN => {
            interrupts::without_interrupts(|| vga_buffer::WRITER.lock().clear_screen())
        }
        _ => return Err(Error::InvalidArgument),
    }
    Ok(0)
}

fn write(fd: usize, data: &[u8]) -> Result<usize, Error> {
    if fd == STDOUT {
        interrupts::without_interrupts(|| vga_buffer::WRITER.lock().write_bytes(data));
        return Ok(data.len());
    }
    tmpfs::write(file_fd(fd)?, data)
}

fn file_fd(fd: usize) -> Result<usize, Error> {
    fd.checked_sub(FIRST_FILE_FD).ok_or(Error::BadFd)
}

/// Copies an optional record out to the program: 1 if there was one, 0 if not.
fn fill<T: Copy>(ptr: usize, value: Option<T>) -> Result<usize, Error> {
    match value {
        Some(value) => {
            *user_mut::<T>(ptr)? = value;
            Ok(1)
        }
        None => Ok(0),
    }
}

fn exec(a: [usize; 4]) -> Result<usize, Error> {
    let name = user_str(a[0], a[1])?;
    let argv = user_slice::<StrRef>(a[2], a[3])?;
    let args = argv.iter().map(|arg| user_str(arg.ptr as usize, arg.len)).collect::<Result<Vec<_>, _>>()?;
    programs::run(name, &args).map(|exit| exit as usize)
}

// --- Access to program memory. -------------------------------------------
// In ring 0 programs share the kernel's address space, so these only reject
// null and misaligned pointers. In ring 3 they must also check that the whole
// range belongs to the calling program.

fn user_slice<'a, T>(ptr: usize, len: usize) -> Result<&'a [T], Error> {
    if len == 0 {
        return Ok(&[]);
    }
    if ptr == 0 || ptr % align_of::<T>() != 0 {
        return Err(Error::InvalidArgument);
    }
    Ok(unsafe { core::slice::from_raw_parts(ptr as *const T, len) })
}

fn user_buf<'a>(ptr: usize, len: usize) -> Result<&'a [u8], Error> {
    user_slice(ptr, len)
}

fn user_buf_mut<'a>(ptr: usize, len: usize) -> Result<&'a mut [u8], Error> {
    if len == 0 {
        return Ok(&mut []);
    }
    if ptr == 0 {
        return Err(Error::InvalidArgument);
    }
    Ok(unsafe { core::slice::from_raw_parts_mut(ptr as *mut u8, len) })
}

fn user_str<'a>(ptr: usize, len: usize) -> Result<&'a str, Error> {
    core::str::from_utf8(user_buf(ptr, len)?).map_err(|_| Error::InvalidArgument)
}

fn user_mut<'a, T>(ptr: usize) -> Result<&'a mut T, Error> {
    if ptr == 0 || ptr % align_of::<T>() != 0 {
        return Err(Error::InvalidArgument);
    }
    Ok(unsafe { &mut *(ptr as *mut T) })
}

