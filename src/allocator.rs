use alloc::alloc::{GlobalAlloc, Layout};
use core::ptr::null_mut;
use linked_list_allocator::LockedHeap;
use x86_64::{
    VirtAddr,
    structures::paging::{FrameAllocator, Mapper, PageTableFlags, Size4KiB, mapper::MapToError},
};

pub const HEAP_START: usize = 0x_4444_4444_0000;
pub const HEAP_SIZE: usize = 1024 * 1024; // 1 MiB

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

pub fn init_heap(
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) -> Result<(), MapToError<Size4KiB>> {
    crate::memory::map_region(
        mapper,
        frame_allocator,
        VirtAddr::new(HEAP_START as u64),
        HEAP_SIZE as u64,
        PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
    )?;

    unsafe {
        ALLOCATOR.lock().init(HEAP_START, HEAP_SIZE);
    }

    Ok(())
}

pub struct Dummy;

unsafe impl GlobalAlloc for Dummy {
    unsafe fn alloc(&self, _layout: Layout) -> *mut u8 {
        null_mut()
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        panic!("dealloc should be never called")
    }
}
