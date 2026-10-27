use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;
use x86_64::instructions::interrupts;

pub const CTRL_C: char = '\u{3}';
pub const CTRL_D: char = '\u{4}';

const CAPACITY: usize = 128;

// Fixed-size ring buffer: the producer is the keyboard interrupt handler,
// where allocating on the heap could deadlock on the allocator lock.
struct RingBuffer {
    data: [char; CAPACITY],
    head: usize,
    len: usize,
}

impl RingBuffer {
    const fn new() -> Self {
        RingBuffer {
            data: ['\0'; CAPACITY],
            head: 0,
            len: 0,
        }
    }

    fn push(&mut self, c: char) {
        if self.len == CAPACITY {
            return; // drop keystrokes when nobody is reading
        }
        self.data[(self.head + self.len) % CAPACITY] = c;
        self.len += 1;
    }

    fn pop(&mut self) -> Option<char> {
        if self.len == 0 {
            return None;
        }
        let c = self.data[self.head];
        self.head = (self.head + 1) % CAPACITY;
        self.len -= 1;
        Some(c)
    }
}

static QUEUE: Mutex<RingBuffer> = Mutex::new(RingBuffer::new());
static STOP_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Called from the keyboard interrupt handler (interrupts are already disabled there).
pub(crate) fn push_key(c: char) {
    if c == CTRL_C {
        STOP_REQUESTED.store(true, Ordering::SeqCst);
    }
    QUEUE.lock().push(c);
}

pub fn try_pop() -> Option<char> {
    // The interrupt handler takes the same lock, so it must not fire while we hold it.
    interrupts::without_interrupts(|| QUEUE.lock().pop())
}

/// Blocks until a key is available, sleeping the CPU between interrupts.
pub fn pop_blocking() -> char {
    loop {
        interrupts::disable();
        if let Some(c) = QUEUE.lock().pop() {
            interrupts::enable();
            return c;
        }
        // `sti; hlt` is atomic: a key arriving right now still wakes us up.
        interrupts::enable_and_hlt();
    }
}

pub fn stop_requested() -> bool {
    STOP_REQUESTED.load(Ordering::SeqCst)
}

pub fn clear_stop() {
    STOP_REQUESTED.store(false, Ordering::SeqCst);
}
