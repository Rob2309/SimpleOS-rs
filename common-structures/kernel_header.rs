
#[repr(C)]
pub struct KernelHeader {
    pub screen_buffer: *mut u8,
    pub screen_width: u32,
    pub screen_height: u32,
    pub screen_scanline_width: u32,
}
