use alloc::vec::Vec;
use bootloader::bootinfo::{MemoryMap, MemoryRegionType};
use spin::Mutex;
use x86_64::{
    PhysAddr, VirtAddr,
    structures::paging::{
        FrameAllocator, FrameDeallocator, Mapper, OffsetPageTable, Page, PageTable,
        PageTableFlags, PhysFrame, Size4KiB, mapper::MapToError,
    },
};

/// What the kernel keeps after boot to manage memory at run time: where all
/// of physical memory is mapped, and the frame allocator.
pub struct Memory {
    pub phys_offset: VirtAddr,
    pub frames: BootInfoFrameAllocator,
    /// The kernel's own level 4 table, the template for every address space.
    pub kernel_p4: PhysFrame,
}

static MEMORY: Mutex<Option<Memory>> = Mutex::new(None);

pub(crate) fn install(memory: Memory) {
    *MEMORY.lock() = Some(memory);
}

pub(crate) fn with<T>(f: impl FnOnce(&mut Memory) -> T) -> T {
    f(MEMORY.lock().as_mut().expect("memory not initialized"))
}

/// Physical frames handed out at run time and not given back yet; lets tests
/// check that running programs does not leak memory.
pub fn frames_in_use() -> usize {
    MEMORY.lock().as_ref().map_or(0, |m| m.frames.in_use)
}

impl Memory {
    /// The page table stored in `frame`, through the physical memory mapping.
    ///
    /// # Safety
    /// `frame` must hold a page table, and no other reference to it may be live.
    pub unsafe fn table(&self, frame: PhysFrame) -> &'static mut PageTable {
        unsafe { &mut *(self.phys_offset + frame.start_address().as_u64()).as_mut_ptr() }
    }

    /// A frame filled with zeros.
    pub fn zeroed_frame(&mut self) -> Option<PhysFrame> {
        let frame = self.frames.allocate_frame()?;
        let ptr: *mut u8 = (self.phys_offset + frame.start_address().as_u64()).as_mut_ptr();
        unsafe { core::ptr::write_bytes(ptr, 0, 4096) };
        Some(frame)
    }
}

/// Flags of the page containing `addr` in the active address space, if it is
/// mapped and every level of the walk allows ring 3 access.
pub fn user_page_flags(addr: VirtAddr) -> Option<PageTableFlags> {
    use x86_64::registers::control::Cr3;

    let phys_offset = MEMORY.lock().as_ref()?.phys_offset;
    let (mut frame, _) = Cr3::read();
    let indices = [addr.p4_index(), addr.p3_index(), addr.p2_index(), addr.p1_index()];
    let mut flags = PageTableFlags::empty();
    for index in indices {
        let table: &PageTable = unsafe { &*(phys_offset + frame.start_address().as_u64()).as_ptr() };
        let entry = &table[index];
        flags = entry.flags();
        if !flags.contains(PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE)
            || flags.contains(PageTableFlags::HUGE_PAGE)
        {
            return None;
        }
        frame = PhysFrame::containing_address(entry.addr());
    }
    Some(flags)
}

/// Backs the virtual range `start..start + size` with fresh physical frames.
pub fn map_region(
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
    start: VirtAddr,
    size: u64,
    flags: PageTableFlags,
) -> Result<(), MapToError<Size4KiB>> {
    let start_page = Page::containing_address(start);
    let end_page = Page::containing_address(start + size - 1u64);

    for page in Page::range_inclusive(start_page, end_page) {
        let frame = frame_allocator
            .allocate_frame()
            .ok_or(MapToError::FrameAllocationFailed)?;
        unsafe { mapper.map_to(page, frame, flags, frame_allocator)?.flush() };
    }
    Ok(())
}

/// Initialize a new OffsetPageTable.
///
/// This function is unsafe because the caller must guarantee that the
/// complete physical memory is mapped to virtual memory at the passed
/// `physical_memory_offset`. Also, this function must be only called once
/// to avoid aliasing `&mut` references (which is undefined behavior).
pub unsafe fn init(physical_memory_offset: VirtAddr) -> OffsetPageTable<'static> {
    unsafe {
        let level_4_table = active_level_4_table(physical_memory_offset);
        OffsetPageTable::new(level_4_table, physical_memory_offset)
    }
}

/// Returns a mutable reference to the active level 4 table.
///
/// This function is unsafe because the caller must guarantee that the
/// complete physical memory is mapped to virtual memory at the passed
/// `physical_memory_offset`. Also, this function must be only called once
/// to avoid aliasing `&mut` references (which is undefined behavior).
unsafe fn active_level_4_table(physical_memory_offset: VirtAddr) -> &'static mut PageTable {
    use x86_64::registers::control::Cr3;

    let (level_4_table_frame, _) = Cr3::read();

    let phys = level_4_table_frame.start_address();
    let virt = physical_memory_offset + phys.as_u64();
    let page_table_ptr: *mut PageTable = virt.as_mut_ptr();

    unsafe { &mut *page_table_ptr }
}

/// A FrameAllocator that always returns `None`.
pub struct EmptyFrameAllocator;

unsafe impl FrameAllocator<Size4KiB> for EmptyFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        None
    }
}

/// A FrameAllocator that returns usable frames from the bootloader's memory
/// map, reusing frames given back through [`FrameDeallocator`] first.
pub struct BootInfoFrameAllocator {
    memory_map: &'static MemoryMap,
    next: usize,
    free: Vec<PhysFrame>,
    in_use: usize,
}

impl BootInfoFrameAllocator {
    /// Create a FrameAllocator from the passed memory map.
    ///
    /// This function is unsafe because the caller must guarantee that the passed
    /// memory map is valid. The main requirement is that all frames that are marked
    /// as `USABLE` in it are really unused.
    pub unsafe fn init(memory_map: &'static MemoryMap) -> Self {
        BootInfoFrameAllocator {
            memory_map,
            next: 0,
            // Empty until the first deallocation, so it is fine to create
            // this before the heap exists.
            free: Vec::new(),
            in_use: 0,
        }
    }

    /// Returns an iterator over the usable frames specified in the memory map.
    fn usable_frames(&self) -> impl Iterator<Item = PhysFrame> {
        // get usable regions from memory map
        let regions = self.memory_map.iter();
        let usable_regions = regions.filter(|r| r.region_type == MemoryRegionType::Usable);
        // map each region to its address range
        let addr_ranges = usable_regions.map(|r| r.range.start_addr()..r.range.end_addr());
        // transform to an iterator of frame start addresses
        let frame_addresses = addr_ranges.flat_map(|r| r.step_by(4096));
        // create `PhysFrame` types from the start addresses
        frame_addresses.map(|addr| PhysFrame::containing_address(PhysAddr::new(addr)))
    }
}

unsafe impl FrameAllocator<Size4KiB> for BootInfoFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        let frame = self.free.pop().or_else(|| {
            let frame = self.usable_frames().nth(self.next);
            self.next += 1;
            frame
        })?;
        self.in_use += 1;
        Some(frame)
    }
}

impl FrameDeallocator<Size4KiB> for BootInfoFrameAllocator {
    unsafe fn deallocate_frame(&mut self, frame: PhysFrame) {
        self.free.push(frame);
        self.in_use -= 1;
    }
}
