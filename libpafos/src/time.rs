use crate::raw::syscall_noptr;
use pafos_abi::nr;

/// Milliseconds since boot.
pub fn uptime_ms() -> u64 {
    syscall_noptr(nr::UPTIME_MS, [0; 4]).unwrap_or(0) as u64
}
