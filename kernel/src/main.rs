#![no_std]
#![no_main]

use core::{panic::PanicInfo, slice};

use common_structures::KernelHeader;
use memory::PhysMemoryManager;

mod mutex;
mod memory;

/// The kernel entry point.
/// This function will be called by the bootloader after preparing the environment.
#[no_mangle]
extern "C" fn _start(kernel_header: *const KernelHeader) -> ! {
    let kh = unsafe{&*kernel_header};

    PhysMemoryManager::init(kh);

    let pixels = unsafe { slice::from_raw_parts_mut(kh.screen_buffer, kh.screen_scanline_width as usize * kh.screen_height as usize * 4) };

    let mut color = 0u8;

    loop {
        for x in 500..kh.screen_width.min(550) {
            for y in 500..kh.screen_height.min(550) {
                pixels[((x + y * kh.screen_scanline_width) * 4) as usize    ] = color;
                pixels[((x + y * kh.screen_scanline_width) * 4) as usize + 1] = y as u8;
                pixels[((x + y * kh.screen_scanline_width) * 4) as usize + 2] = 0x00;
            }
        }
        
        color = color.wrapping_add(1);
    }
}

/// Will be called by functions like panic!(), expect(), unwrap(), etc. when errors occur.
#[panic_handler]
fn panic_handler(_info: &PanicInfo) -> ! {
    // Since we have no printing functionality yet, we just loop and cry :(
    loop {}
}
