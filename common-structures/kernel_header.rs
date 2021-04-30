
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
}
