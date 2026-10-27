use crate::{gdt, hlt_loop, println, process};
use lazy_static::lazy_static;
use pic8259::ChainedPics;
use spin;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};
use x86_64::{PrivilegeLevel, VirtAddr};

pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptIndex {
    Timer = PIC_1_OFFSET,
    Keyboard,
}

impl InterruptIndex {
    fn as_u8(self) -> u8 {
        self as u8
    }
}

pub static PICS: spin::Mutex<ChainedPics> =
    spin::Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

lazy_static! {
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        idt.page_fault.set_handler_fn(page_fault_handler);
        idt.divide_error.set_handler_fn(divide_error_handler);
        idt.invalid_opcode.set_handler_fn(invalid_opcode_handler);
        idt.general_protection_fault.set_handler_fn(general_protection_fault_handler);
        idt.stack_segment_fault.set_handler_fn(stack_segment_fault_handler);
        unsafe {
            idt.double_fault
                .set_handler_fn(double_fault_handler)
                .set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);
        }
        idt[InterruptIndex::Timer.as_u8()].set_handler_fn(timer_interrupt_handler);
        idt[InterruptIndex::Keyboard.as_u8()].set_handler_fn(keyboard_interrupt_handler);
        // DPL 3 so ring 3 code may `int 0x80`; a trap gate (interrupts stay
        // enabled) because a system call can run for long, e.g. `exec`.
        unsafe {
            idt[pafos_abi::SYSCALL_VECTOR]
                .set_handler_addr(VirtAddr::new(crate::syscall::entry as *const () as u64))
                .set_privilege_level(PrivilegeLevel::Ring3)
                .disable_interrupts(false);
        }
        idt
    };
}

pub fn init_idt() {
    IDT.load();
}

extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    println!("EXCEPTION: BREAKPOINT\n{:#?}", stack_frame);
}

/// True if the CPU was running ring 3 code when the exception happened.
/// Such a fault is the program's problem: it gets killed, the kernel goes on.
fn from_user(stack_frame: &InterruptStackFrame) -> bool {
    stack_frame.code_segment.rpl() == PrivilegeLevel::Ring3
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    use x86_64::registers::control::Cr2;

    if from_user(&stack_frame) {
        let addr = Cr2::read().map_or(0, |a| a.as_u64());
        process::kill(format_args!("page fault at {:#x}", addr));
    }
    println!("EXCEPTION: PAGE FAULT");
    println!("Accessed Address: {:?}", Cr2::read());
    println!("Error Code: {:?}", error_code);
    println!("{:#?}", stack_frame);
    hlt_loop();
}

extern "x86-interrupt" fn divide_error_handler(stack_frame: InterruptStackFrame) {
    if from_user(&stack_frame) {
        process::kill(format_args!("division by zero"));
    }
    panic!("EXCEPTION: DIVIDE ERROR
{:#?}", stack_frame);
}

extern "x86-interrupt" fn invalid_opcode_handler(stack_frame: InterruptStackFrame) {
    if from_user(&stack_frame) {
        process::kill(format_args!("invalid instruction at {:#x}", stack_frame.instruction_pointer.as_u64()));
    }
    panic!("EXCEPTION: INVALID OPCODE
{:#?}", stack_frame);
}

extern "x86-interrupt" fn general_protection_fault_handler(stack_frame: InterruptStackFrame, error_code: u64) {
    if from_user(&stack_frame) {
        process::kill(format_args!("protection fault at {:#x}", stack_frame.instruction_pointer.as_u64()));
    }
    panic!("EXCEPTION: GENERAL PROTECTION FAULT ({:#x})
{:#?}", error_code, stack_frame);
}

extern "x86-interrupt" fn stack_segment_fault_handler(stack_frame: InterruptStackFrame, error_code: u64) {
    if from_user(&stack_frame) {
        process::kill(format_args!("stack fault"));
    }
    panic!("EXCEPTION: STACK SEGMENT FAULT ({:#x})
{:#?}", error_code, stack_frame);
}

extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame,
    _error_code: u64,
) -> ! {
    panic!("EXCEPTION: DOUBLE FAULT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn timer_interrupt_handler(_stack_frame: InterruptStackFrame) {
    crate::clock::tick();
    unsafe {
        PICS.lock()
            .notify_end_of_interrupt(InterruptIndex::Timer.as_u8());
    }
}

extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    use pc_keyboard::{DecodedKey, HandleControl, Keyboard, ScancodeSet1, layouts};
    use spin::Mutex;
    use x86_64::instructions::port::Port;

    static KEYBOARD: Mutex<Keyboard<layouts::Us104Key, ScancodeSet1>> = Mutex::new(Keyboard::new(
        ScancodeSet1::new(),
        layouts::Us104Key,
        // Ctrl+<letter> arrives as a control char (Ctrl+C = '\x03', Ctrl+D = '\x04').
        HandleControl::MapLettersToUnicode,
    ));

    let mut keyboard = KEYBOARD.lock();
    let mut port = Port::new(0x60);

    let scancode: u8 = unsafe { port.read() };
    if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
        if let Some(DecodedKey::Unicode(character)) = keyboard.process_keyevent(key_event) {
            crate::input::push_key(character);
        }
    }

    unsafe {
        PICS.lock()
            .notify_end_of_interrupt(InterruptIndex::Keyboard.as_u8());
    }
}

#[test_case]
fn test_breakpoint_exception() {
    // invoke a breakpoint exception
    x86_64::instructions::interrupts::int3();
}
