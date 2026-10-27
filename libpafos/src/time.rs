use crate::raw::syscall_noptr;
use core::fmt;
use pafos_abi::nr;

/// Milliseconds since boot.
pub fn uptime_ms() -> u64 {
    syscall_noptr(nr::UPTIME_MS, [0; 4]).unwrap_or(0) as u64
}

/// Displays a millisecond count in words, rounded down to whole seconds:
/// `14 seconds`, `1 minute 3 seconds`, `2 hours 1 second`.
pub struct Elapsed(pub u64);

impl fmt::Display for Elapsed {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let total = self.0 / 1000;
        let parts = [(total / 3600, "hour"), (total / 60 % 60, "minute"), (total % 60, "second")];
        let mut first = true;
        for (value, unit) in parts {
            if value == 0 && !(unit == "second" && first) {
                continue;
            }
            if !first {
                f.write_str(" ")?;
            }
            write!(f, "{} {}{}", value, unit, if value == 1 { "" } else { "s" })?;
            first = false;
        }
        Ok(())
    }
}
