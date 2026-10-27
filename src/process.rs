//! Ring 3 processes: a program loaded from an ELF image into its own
//! address space.
//!
//! The address space is a fresh level 4 table that shares every kernel entry
//! (so interrupts and system calls work after the CR3 switch) and adds one
//! entry of its own: the user region from `pafos_abi::layout`. Kernel pages
//! lack the USER bit, so ring 3 code cannot touch them.
//!
//! `run` is synchronous: it enters ring 3 with `iretq` and comes back when the
//! program calls EXIT or the CPU faults in ring 3. Both paths jump back to the
//! kernel stack pointer saved on entry, discarding the frames of the trap.
//!
//! Programs nest without any scheduling: when a program calls EXEC, the kernel
//! frames of that system call stay on the kernel stack and the child is
//! entered from there, with RSP0 set below them (see `gdt::rsp0_slot`). Each
//! level has its own saved stack pointer; EXIT and faults always return to
//! the innermost one.

use crate::memory::{self, Memory};
use crate::{gdt, println};
use alloc::string::String;
use alloc::vec::Vec;
use core::arch::naked_asm;
use core::fmt;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use pafos_abi::layout::{USER_BASE, USER_END, USER_STACK_SIZE, USER_STACK_TOP};
use pafos_abi::{Error, Exit, MAX_ARGS, MAX_NESTING, Name, StrRef};
use spin::Mutex;
use x86_64::registers::control::Cr3;
use x86_64::structures::paging::{
    FrameDeallocator, Mapper, OffsetPageTable, Page, PageTableFlags, PhysFrame, Size4KiB,
};
use x86_64::{VirtAddr, instructions::interrupts};

/// Number of ring 3 programs currently started (each waiting for the next,
/// except the innermost one, which runs).
static DEPTH: AtomicUsize = AtomicUsize::new(0);
/// Per level: kernel stack pointer saved by `enter_user`, for `resume_kernel`.
static KERNEL_RSP: [AtomicU64; MAX_NESTING] = [const { AtomicU64::new(0) }; MAX_NESTING];
/// Per level: program name, for messages.
static NAMES: Mutex<[Name; MAX_NESTING]> = Mutex::new([Name::EMPTY; MAX_NESTING]);

const USER_P4_INDEX: usize = (USER_BASE >> 39) as usize & 0x1ff;
/// Argument strings plus the `StrRef` array must fit in this part of the stack.
const MAX_ARGS_BYTES: u64 = 4096;

pub fn run(name: &str, image: &[u8], args: &[&str]) -> Result<Exit, Error> {
    let level = DEPTH.load(Ordering::SeqCst);
    if level == MAX_NESTING {
        return Err(Error::TooDeep);
    }
    let elf = Elf::parse(image).ok_or(Error::InvalidArgument)?;
    if args.len() > MAX_ARGS {
        return Err(Error::InvalidArgument);
    }
    // The arguments may live in the calling program's memory, which is not
    // mapped any more once the child's address space is active.
    let args: Vec<String> = args.iter().map(|&arg| String::from(arg)).collect();

    NAMES.lock()[level] = Name::new(name);
    DEPTH.store(level + 1, Ordering::SeqCst);
    let result = run_in_new_space(level, image, &elf, &args);
    DEPTH.store(level, Ordering::SeqCst);
    result
}

fn run_in_new_space(level: usize, image: &[u8], elf: &Elf, args: &[String]) -> Result<Exit, Error> {
    // The caller's address space: the kernel's, or the parent program's.
    let (parent_p4, cr3_flags) = Cr3::read();
    let p4 = memory::with(new_address_space).ok_or(Error::NoSpace)?;
    unsafe { Cr3::write(p4, cr3_flags) };

    let result = memory::with(|m| load(m, p4, elf, image)).and_then(|()| {
        memory::with(|m| map_stack(m, p4))?;
        let (rsp, argv) = push_args(args)?;
        let (code, data) = gdt::user_selectors();
        let entry = UserEntry {
            rip: elf.entry,
            rsp,
            cs: code.0 as u64,
            ss: data.0 as u64,
            rflags: 0x202, // IF set, plus the always-one bit 1
            argc: args.len() as u64,
            argv,
        };
        let rsp0 = gdt::rsp0_slot();
        let parent_rsp0 = unsafe { rsp0.read_unaligned() };
        let code = unsafe { enter_user(&entry, KERNEL_RSP[level].as_ptr(), rsp0) };
        // Back at this level: the parent's traps go above our frames again.
        unsafe { rsp0.write_unaligned(parent_rsp0) };
        // A fault handler may have brought us back with interrupts off.
        interrupts::enable();
        Ok(Exit::from_raw(code as usize))
    });

    unsafe { Cr3::write(parent_p4, cr3_flags) };
    memory::with(|m| free_address_space(m, p4));
    result
}

/// Called for the EXIT system call from ring 3: ends the innermost program.
pub fn exit(code: usize) -> ! {
    let level = DEPTH.load(Ordering::SeqCst) - 1;
    unsafe { resume_kernel(KERNEL_RSP[level].load(Ordering::SeqCst), code as u64) }
}

/// Called by CPU exception handlers when the fault happened in ring 3.
pub fn kill(reason: fmt::Arguments) -> ! {
    let name = NAMES.lock()[DEPTH.load(Ordering::SeqCst) - 1];
    println!("{}: killed ({})", name.as_str(), reason);
    exit(Exit::Crashed as usize)
}

// --- Address space ----------------------------------------------------------

fn new_address_space(m: &mut Memory) -> Option<PhysFrame> {
    let p4 = m.zeroed_frame()?;
    // From the kernel's table, not the active one: that may be the parent
    // program's, with its user region in it.
    let (new, kernel) = unsafe { (m.table(p4), m.table(m.kernel_p4)) };
    for (i, entry) in kernel.iter().enumerate() {
        new[i] = entry.clone();
    }
    // The kernel must leave this entry to user programs.
    assert!(new[USER_P4_INDEX].is_unused(), "kernel mapping in the user region");
    Some(p4)
}

/// Gives back every frame of the user region and the level 4 table itself.
fn free_address_space(m: &mut Memory, p4: PhysFrame) {
    fn free_table(m: &mut Memory, table: PhysFrame, level: u8) {
        let entries = unsafe { m.table(table) };
        for entry in entries.iter().filter(|e| !e.is_unused()) {
            let frame = PhysFrame::containing_address(entry.addr());
            if level > 1 {
                free_table(m, frame, level - 1);
            } else {
                unsafe { m.frames.deallocate_frame(frame) };
            }
        }
        unsafe { m.frames.deallocate_frame(table) };
    }

    let user = &unsafe { m.table(p4) }[USER_P4_INDEX];
    if !user.is_unused() {
        free_table(m, PhysFrame::containing_address(user.addr()), 3);
    }
    unsafe { m.frames.deallocate_frame(p4) };
}

fn mapper(m: &Memory, p4: PhysFrame) -> OffsetPageTable<'static> {
    unsafe { OffsetPageTable::new(m.table(p4), m.phys_offset) }
}

const USER_TABLE_FLAGS: PageTableFlags = PageTableFlags::PRESENT
    .union(PageTableFlags::WRITABLE)
    .union(PageTableFlags::USER_ACCESSIBLE);

/// The frame behind `page`, mapping a zeroed one first if needed. Two segments
/// may share a page, so an existing mapping is reused (and made writable if
/// the new segment needs it).
fn user_frame(m: &mut Memory, p4: PhysFrame, page: Page, writable: bool) -> Result<PhysFrame, Error> {
    let mut flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
    if writable {
        flags |= PageTableFlags::WRITABLE;
    }
    let mut mapper = mapper(m, p4);
    if let Ok(frame) = mapper.translate_page(page) {
        if writable {
            unsafe { mapper.update_flags(page, flags).map_err(|_| Error::NoSpace)?.flush() };
        }
        return Ok(frame);
    }
    let frame = m.zeroed_frame().ok_or(Error::NoSpace)?;
    let mut mapper = self::mapper(m, p4);
    unsafe {
        mapper
            .map_to_with_table_flags(page, frame, flags, USER_TABLE_FLAGS, &mut m.frames)
            .map_err(|_| Error::NoSpace)?
            .flush()
    };
    Ok(frame)
}

fn load(m: &mut Memory, p4: PhysFrame, elf: &Elf, image: &[u8]) -> Result<(), Error> {
    let stack_bottom = USER_STACK_TOP - USER_STACK_SIZE;
    for seg in elf.segments(image) {
        let seg = seg.ok_or(Error::InvalidArgument)?;
        let end = seg.vaddr.checked_add(seg.memsz).ok_or(Error::InvalidArgument)?;
        // Leave an unmapped guard page between the program and its stack.
        if seg.vaddr < USER_BASE || end > stack_bottom - 4096 || seg.filesz > seg.memsz {
            return Err(Error::InvalidArgument);
        }
        if seg.memsz == 0 {
            continue;
        }
        let first = Page::<Size4KiB>::containing_address(VirtAddr::new(seg.vaddr));
        let last = Page::containing_address(VirtAddr::new(end - 1));
        for page in Page::range_inclusive(first, last) {
            let frame = user_frame(m, p4, page, seg.writable)?;
            // Copy through the physical memory mapping: read-only pages are
            // not writable even for the kernel (CR0.WP).
            let page_start = page.start_address().as_u64();
            let from = seg.vaddr.max(page_start);
            let to = (seg.vaddr + seg.filesz).min(page_start + 4096);
            if from < to {
                let src = &image[(seg.offset + from - seg.vaddr) as usize..(seg.offset + to - seg.vaddr) as usize];
                let dst = (m.phys_offset + frame.start_address().as_u64() + (from - page_start)).as_mut_ptr();
                unsafe { core::ptr::copy_nonoverlapping(src.as_ptr(), dst, src.len()) };
            }
        }
    }
    Ok(())
}

fn map_stack(m: &mut Memory, p4: PhysFrame) -> Result<(), Error> {
    let first = Page::containing_address(VirtAddr::new(USER_STACK_TOP - USER_STACK_SIZE));
    let last = Page::containing_address(VirtAddr::new(USER_STACK_TOP - 1));
    for page in Page::range_inclusive(first, last) {
        user_frame(m, p4, page, true)?;
    }
    Ok(())
}

/// Copies the arguments to the top of the (already active) user stack.
/// Returns the initial stack pointer and the address of the `StrRef` array.
fn push_args(args: &[String]) -> Result<(u64, u64), Error> {
    let total: u64 = args.iter().map(|a| a.len() as u64 + 16).sum();
    if total > MAX_ARGS_BYTES {
        return Err(Error::InvalidArgument);
    }
    let mut sp = USER_STACK_TOP;
    let mut refs = Vec::with_capacity(args.len());
    for arg in args {
        sp -= arg.len() as u64;
        unsafe { core::ptr::copy_nonoverlapping(arg.as_ptr(), sp as *mut u8, arg.len()) };
        refs.push(StrRef { ptr: sp as *const u8, len: arg.len() });
    }
    sp &= !15;
    sp -= (refs.len() * size_of::<StrRef>()) as u64;
    let argv = sp;
    unsafe { core::ptr::copy_nonoverlapping(refs.as_ptr(), argv as *mut StrRef, refs.len()) };
    // As if `_start` had been called: RSP is 8 mod 16 at function entry.
    sp -= 8;
    unsafe { (sp as *mut u64).write(0) };
    Ok((sp, argv))
}

// --- Entering and leaving ring 3 ------------------------------------------

#[repr(C)]
struct UserEntry {
    rip: u64,
    rsp: u64,
    cs: u64,
    ss: u64,
    rflags: u64,
    argc: u64,
    argv: u64,
}

/// Saves the callee-saved registers and RSP to `*saved_rsp`, makes the same
/// RSP the program's RSP0 (`*rsp0`), then `iretq`s into ring 3. "Returns" the
/// exit code when `resume_kernel` jumps back.
#[unsafe(naked)]
unsafe extern "C" fn enter_user(entry: *const UserEntry, saved_rsp: *mut u64, rsp0: *mut u64) -> u64 {
    naked_asm!(
        "push rbx",
        "push rbp",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        "mov [rsi], rsp",
        // The program's traps start below everything pushed so far (the
        // iretq frame pushed next is consumed before any trap can happen).
        "mov [rdx], rsp",
        // Interrupt frame for iretq: SS, RSP, RFLAGS, CS, RIP.
        "push qword ptr [rdi + 24]",
        "push qword ptr [rdi + 8]",
        "push qword ptr [rdi + 32]",
        "push qword ptr [rdi + 16]",
        "push qword ptr [rdi + 0]",
        "mov rsi, [rdi + 48]",
        "mov rdi, [rdi + 40]",
        // Leave no kernel values in registers.
        "xor eax, eax",
        "xor ebx, ebx",
        "xor ecx, ecx",
        "xor edx, edx",
        "xor ebp, ebp",
        "xor r8d, r8d",
        "xor r9d, r9d",
        "xor r10d, r10d",
        "xor r11d, r11d",
        "xor r12d, r12d",
        "xor r13d, r13d",
        "xor r14d, r14d",
        "xor r15d, r15d",
        "iretq",
    );
}

/// Switches back to the stack saved by `enter_user` and returns from it with `code`.
#[unsafe(naked)]
unsafe extern "C" fn resume_kernel(saved_rsp: u64, code: u64) -> ! {
    naked_asm!(
        "mov rsp, rdi",
        "mov rax, rsi",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbp",
        "pop rbx",
        "ret",
    );
}

// --- ELF --------------------------------------------------------------------

/// The parts of a 64-bit ELF executable the loader needs.
struct Elf {
    entry: u64,
    phoff: usize,
    phentsize: usize,
    phnum: usize,
}

struct Segment {
    offset: u64,
    vaddr: u64,
    filesz: u64,
    memsz: u64,
    writable: bool,
}

const PT_LOAD: u32 = 1;
const PF_W: u32 = 2;

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}
fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}
fn u64_at(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

impl Elf {
    fn parse(image: &[u8]) -> Option<Elf> {
        // Magic, 64-bit, little-endian; executable (ET_EXEC) for x86_64.
        if image.get(0..6)? != b"\x7fELF\x02\x01" || u16_at(image, 16)? != 2 || u16_at(image, 18)? != 0x3e {
            return None;
        }
        let entry = u64_at(image, 24)?;
        if !(USER_BASE..USER_END).contains(&entry) {
            return None;
        }
        Some(Elf {
            entry,
            phoff: u64_at(image, 32)? as usize,
            phentsize: u16_at(image, 54)? as usize,
            phnum: u16_at(image, 56)? as usize,
        })
    }

    /// Loadable segments; `None` items for malformed headers.
    fn segments<'a>(&'a self, image: &'a [u8]) -> impl Iterator<Item = Option<Segment>> + 'a {
        (0..self.phnum).filter_map(move |i| {
            let at = self.phoff.checked_add(i.checked_mul(self.phentsize)?)?;
            let header = (|| {
                let kind = u32_at(image, at)?;
                let seg = Segment {
                    writable: u32_at(image, at + 4)? & PF_W != 0,
                    offset: u64_at(image, at + 8)?,
                    vaddr: u64_at(image, at + 16)?,
                    filesz: u64_at(image, at + 32)?,
                    memsz: u64_at(image, at + 40)?,
                };
                let in_file = seg.offset.checked_add(seg.filesz)? <= image.len() as u64;
                Some((kind, in_file.then_some(seg)))
            })();
            match header {
                Some((PT_LOAD, seg)) => Some(seg),
                Some(_) => None,
                None => Some(None),
            }
        })
    }
}
