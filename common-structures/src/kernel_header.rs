
/// A structure containing various information passed to the kernel entry point
#[repr(C)]
pub struct KernelHeader {
    /// Pointer to the GPU framebuffer.
    /// Can be used to draw to the screen
    pub screen_buffer: *mut u8,
    /// The framebuffer width in pixels
    pub screen_width: u32,
    /// The framebuffer height in pixels
    pub screen_height: u32,
    /// The width of a scanline in pixels.
    pub screen_scanline_width: u32,

    // Platform dependent Page Table information
    pub paging_info: PagingInfo,

    pub memory_map: *mut MemorySegment,
    pub memory_map_entries: u64,
}

#[repr(C)]
pub struct MemorySegment {
    pub start: u64,
    pub page_count: u64,
    pub state: MemorySegmentState,
}

#[repr(C)]
pub enum MemorySegmentState {
    Free,
    Occupied,
}

#[cfg(target_arch="x86_64")]
#[repr(C)]
pub struct PagingInfo {
    /// Pointer to the initial page table.
    /// 
    /// The table will have an identity mapping of physical memory
    /// as well as a mirror in the higher memory half.
    pub page_buffer: *mut u64,
    /// Number of pages used for the Page Directory Pointer Tables
    pub pdp_pages: u64,
    /// Number of pages used for the Page Directory Tables
    pub pd_pages: u64,
}
