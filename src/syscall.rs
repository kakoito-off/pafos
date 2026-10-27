//! System calls: the only way programs reach the kernel.
//!
//! A program executes `int 0x80` (see `pafos_abi::SYSCALL_VECTOR` for the
//! register convention). [`entry`] saves registers and hands them to
//! [`dispatch`], together with the privilege level of the caller. Calls from
//! ring 3 arrive on the TSS's RSP0 stack, and every pointer they pass is
//! checked against the program's own mappings before the kernel touches it.
//! Programs linked into the kernel (ring 0) are trusted.

use crate::{clock, input, memory, process, programs, tmpfs, vga_buffer};
use alloc::vec::Vec;
use core::arch::naked_asm;
use pafos_abi::layout::{USER_BASE, USER_END};
use pafos_abi::{
    Error, FIRST_FILE_FD, FsStats, Mode, READ_KEY_NONBLOCK, STDOUT, StrRef, console, encode, nr,
};
use x86_64::VirtAddr;
use x86_64::instructions::interrupts;
use x86_64::structures::paging::PageTableFlags;

/// Handler for the system call vector.
///
/// The CPU has pushed the interrupt frame (SS, RSP, RFLAGS, CS, RIP), which
/// leaves RSP at 8 mod 16. Caller-saved registers are saved here because the
/// ABI promises the program that only `rax` changes; callee-saved ones are
/// preserved by `handle` itself.
#[unsafe(naked)]
pub extern "C" fn entry() {
    naked_asm!(
        "push rcx",
        "push rdx",
        "push rsi",
        "push rdi",
        "push r8",
        "push r9",
        "push r10",
        "push r11",
        // 5 + 8 pushes so far: realign to 16 bytes for the call.
        "sub rsp, 8",
        // The C ABI requires the direction flag clear; the program may have set it.
        "cld",
        // Caller's CS from the interrupt frame (after 72 bytes of pushes: RIP, then CS).
        "mov r9, [rsp + 80]",
        // Syscall registers -> C arguments: handle(rax, rdi, rsi, rdx, r10, cs).
        "mov r8, r10",
        "mov rcx, rdx",
        "mov rdx, rsi",
        "mov rsi, rdi",
        "mov rdi, rax",
        "call {handle}",
        "add rsp, 8",
        "pop r11",
        "pop r10",
        "pop r9",
        "pop r8",
        "pop rdi",
        "pop rsi",
        "pop rdx",
        "pop rcx",
        "iretq",
        handle = sym handle,
    );
}

extern "C" fn handle(nr: usize, a0: usize, a1: usize, a2: usize, a3: usize, cs: u64) -> isize {
    let caller = Caller { user: cs & 3 == 3 };
    encode(dispatch(caller, nr, [a0, a1, a2, a3]))
}

fn dispatch(c: Caller, nr: usize, a: [usize; 4]) -> Result<usize, Error> {
    match nr {
        nr::READ_KEY => read_key(a[0]),
        nr::STOP_REQUESTED => Ok(input::stop_requested() as usize),
        nr::CLEAR_STOP => {
            input::clear_stop();
            Ok(0)
        }
        nr::CONSOLE_CTL => console_ctl(a[0]),

        nr::CREATE => tmpfs::create(c.str(a[0], a[1])?).map(|()| 0),
        nr::OPEN => {
            let fd = tmpfs::open(c.str(a[0], a[1])?, Mode::from_raw(a[2])?)?;
            Ok(fd + FIRST_FILE_FD)
        }
        nr::READ => tmpfs::read(file_fd(a[0])?, c.buf_mut(a[1], a[2])?),
        nr::WRITE => write(a[0], c.buf(a[1], a[2])?),
        nr::CLOSE => tmpfs::close(file_fd(a[0])?).map(|()| 0),
        nr::UNLINK => tmpfs::remove(c.str(a[0], a[1])?).map(|()| 0),
        nr::READDIR => fill(c, a[1], tmpfs::file_at(a[0])),
        nr::FSSTAT => {
            *c.out::<FsStats>(a[0])? = tmpfs::stats()?;
            Ok(0)
        }

        nr::UPTIME_MS => Ok(clock::uptime_ms() as usize),

        nr::EXEC => exec(c, a),
        nr::PROGRAM_AT => fill(c, a[1], programs::name_at(a[0])),
        nr::EXIT if c.user => process::exit(a[0]),
        // A ring 0 program has nowhere to exit to; it returns from `main`.
        nr::EXIT => Err(Error::InvalidArgument),
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
fn fill<T: Copy>(c: Caller, ptr: usize, value: Option<T>) -> Result<usize, Error> {
    match value {
        Some(value) => {
            *c.out::<T>(ptr)? = value;
            Ok(1)
        }
        None => Ok(0),
    }
}

fn exec(c: Caller, a: [usize; 4]) -> Result<usize, Error> {
    // Starting programs from ring 3 needs a kernel stack per process.
    if c.user {
        return Err(Error::Busy);
    }
    let name = c.str(a[0], a[1])?;
    let argv = c.slice::<StrRef>(a[2], a[3])?;
    let args = argv.iter().map(|arg| c.str(arg.ptr as usize, arg.len)).collect::<Result<Vec<_>, _>>()?;
    programs::run(name, &args).map(|exit| exit as usize)
}

// --- Access to program memory. -------------------------------------------

/// Who made the call. Programs in ring 0 share the kernel's address space, so
/// their pointers are only checked for null and alignment. For ring 3 the
/// whole range must also be mapped for user access in the program's own
/// region (and writable where the kernel writes), so a bad pointer is an
/// error instead of a kernel page fault or a peek into kernel memory.
#[derive(Clone, Copy)]
struct Caller {
    user: bool,
}

impl Caller {
    fn check(self, ptr: usize, bytes: usize, align: usize, writable: bool) -> Result<(), Error> {
        if ptr == 0 || ptr % align != 0 {
            return Err(Error::InvalidArgument);
        }
        if !self.user {
            return Ok(());
        }
        let start = ptr as u64;
        let end = start.checked_add(bytes as u64).ok_or(Error::InvalidArgument)?;
        if start < USER_BASE || end > USER_END {
            return Err(Error::InvalidArgument);
        }
        let mut page = start & !0xfff;
        while page < end {
            let flags = memory::user_page_flags(VirtAddr::new(page)).ok_or(Error::InvalidArgument)?;
            if writable && !flags.contains(PageTableFlags::WRITABLE) {
                return Err(Error::InvalidArgument);
            }
            page += 4096;
        }
        Ok(())
    }

    fn slice<'a, T>(self, ptr: usize, len: usize) -> Result<&'a [T], Error> {
        if len == 0 {
            return Ok(&[]);
        }
        let bytes = len.checked_mul(size_of::<T>()).ok_or(Error::InvalidArgument)?;
        self.check(ptr, bytes, align_of::<T>(), false)?;
        Ok(unsafe { core::slice::from_raw_parts(ptr as *const T, len) })
    }

    fn buf<'a>(self, ptr: usize, len: usize) -> Result<&'a [u8], Error> {
        self.slice(ptr, len)
    }

    fn buf_mut<'a>(self, ptr: usize, len: usize) -> Result<&'a mut [u8], Error> {
        if len == 0 {
            return Ok(&mut []);
        }
        self.check(ptr, len, 1, true)?;
        Ok(unsafe { core::slice::from_raw_parts_mut(ptr as *mut u8, len) })
    }

    fn str<'a>(self, ptr: usize, len: usize) -> Result<&'a str, Error> {
        core::str::from_utf8(self.buf(ptr, len)?).map_err(|_| Error::InvalidArgument)
    }

    /// A place to copy a record out to.
    fn out<'a, T>(self, ptr: usize) -> Result<&'a mut T, Error> {
        self.check(ptr, size_of::<T>(), align_of::<T>(), true)?;
        Ok(unsafe { &mut *(ptr as *mut T) })
    }
}
