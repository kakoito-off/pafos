//! Time since boot, counted by the PIT (programmable interval timer).

use core::sync::atomic::{AtomicU64, Ordering};
use x86_64::instructions::port::Port;

/// Input clock of the PIT.
const PIT_FREQUENCY: u64 = 1_193_182;
/// ~100 Hz: 10 ms resolution instead of the power-on default of ~18.2 Hz.
const DIVISOR: u16 = 11_932;

static TICKS: AtomicU64 = AtomicU64::new(0);

/// Programs PIT channel 0 to fire IRQ0 at ~100 Hz. Call before enabling interrupts.
pub fn init() {
    let mut command = Port::<u8>::new(0x43);
    let mut channel0 = Port::<u8>::new(0x40);
    unsafe {
        // Channel 0, low byte then high byte, mode 3 (square wave), binary.
        command.write(0x36);
        channel0.write((DIVISOR & 0xff) as u8);
        channel0.write((DIVISOR >> 8) as u8);
    }
}

/// Called from the timer interrupt handler.
pub(crate) fn tick() {
    TICKS.fetch_add(1, Ordering::Relaxed);
}

pub fn uptime_ms() -> u64 {
    TICKS.load(Ordering::Relaxed) * DIVISOR as u64 * 1000 / PIT_FREQUENCY
}
