#![no_std]
#![no_main]
#![feature(abi_efiapi)]
#![feature(option_result_unwrap_unchecked)]
#![feature(panic_info_message)]
#![feature(alloc_error_handler)]
#![feature(asm)]

use core::{alloc::Layout, panic::PanicInfo, slice};

use uefi::{prelude::*, proto::{console::{gop::{GraphicsOutput, PixelFormat}, text::Output}, loaded_image::LoadedImage, media::fs::SimpleFileSystem}, table::boot::{AllocateType, MemoryType}};
use core::fmt::Write;

mod allocator;
mod io;
mod elf;
mod paging;
mod platform;

extern crate alloc;

include!("../../common-structures/kernel_header.rs");

static mut STDOUT: *mut Output = core::ptr::null_mut();
static mut FILESYSTEM: *mut SimpleFileSystem = core::ptr::null_mut();

#[no_mangle]
extern "efiapi" fn efi_main(img_handle: Handle, system_table: SystemTable<Boot>) -> Status {
    unsafe {
        uefi::alloc::init(system_table.boot_services());

        STDOUT = system_table.stdout() as *const _ as *mut _;
    }

    let _ = system_table.stdout().reset(true);
    write!(system_table.stdout(), "Initializing bootloader...\r\n").unwrap();

    let loaded_image = system_table.boot_services().handle_protocol::<LoadedImage>(img_handle).expect("LoadedImageProtocol not found").split().1;
    let file_system = system_table.boot_services().handle_protocol::<SimpleFileSystem>(unsafe{&mut *loaded_image.get()}.device()).expect("SimpleFileSystemProtocol not found").split().1;
    let graphics = system_table.boot_services().locate_protocol::<GraphicsOutput>().expect("GraphicsOutputProtocol not found").split().1;

    unsafe {
        FILESYSTEM = file_system.get();
    }

    let mut kernel_header = allocator::allocate_object::<KernelHeader>(&system_table, MemoryType::LOADER_DATA);

    write!(system_table.stdout(), "Initializing Paging...\r\n").unwrap();

    // initialize page tables so that the higher memory half mirrors the lower half
    paging::init(&system_table);

    // convert kernel_header address to high er half
    kernel_header = unsafe{&mut *paging::ptr_to_kernelspace(kernel_header)};

    write!(system_table.stdout(), "Switching video mode...\r\n").unwrap();

    {
        let gfx = unsafe {&mut *graphics.get()};

        let mut res_best_x = 0;
        let mut res_best_mode = None;
        for m in gfx.modes().map(|m| m.split().1) {
            let info = m.info();

            // restrict to width of 1920, else VMs tend to give huge resolutions
            if info.resolution().0 <= 1920 && info.resolution().0 > res_best_x && (info.pixel_format() == PixelFormat::Bgr || info.pixel_format() == PixelFormat::Rgb) {
                res_best_x = info.resolution().0;
                res_best_mode = Some(m);
            }
        }

        if let Some(m) = res_best_mode {
            let _ = gfx.set_mode(&m).expect("Failed to set video mode");
            kernel_header.screen_width = m.info().resolution().0 as u32;
            kernel_header.screen_height = m.info().resolution().1 as u32;
            kernel_header.screen_scanline_width = m.info().stride() as u32;
        }

        kernel_header.screen_buffer = gfx.frame_buffer().as_mut_ptr();
    }

    write!(system_table.stdout(), "Loading modules...\r\n").unwrap();

    let kernel_image = io::read_file(&system_table, "EFI\\BOOT\\kernel.sys");
    let kernel_elf_size = elf::get_size(kernel_image.data);

    write!(system_table.stdout(), "Kernel size: {}\r\n", kernel_elf_size).unwrap();
    write!(system_table.stdout(), "Preparing kernel...\r\n").unwrap();

    let process_buffer = paging::ptr_to_kernelspace(allocator::allocate(&system_table, kernel_elf_size, MemoryType::LOADER_DATA));
    let entry_point = elf::prepare(kernel_image.data, process_buffer);

    write!(system_table.stdout(), "Kernel at {:016X} (entry point {:016X})\r\n", process_buffer as u64, entry_point).unwrap();

    // Prepare debug marker
    #[cfg(debug_assertions)]
    {
        let debug_data = system_table.boot_services().allocate_pages(AllocateType::Address(0x1000), MemoryType::LOADER_DATA, 1).expect("Failed to allocate debug buffer").split().1 as *mut u64;
        unsafe {
            *debug_data = elf::get_text_addr(kernel_image.data, process_buffer);
        }
    }

    allocator::free(&system_table, kernel_image.data, kernel_image.size as usize);

    write!(system_table.stdout(), "Starting kernel...\r\n").unwrap();

    let mmap_pages = (system_table.boot_services().memory_map_size() + 4095) / 4096 + 1;
    let mmap_buffer = system_table.boot_services().allocate_pages(AllocateType::AnyPages, MemoryType::LOADER_DATA, mmap_pages).expect("Failed to allocate mmap buffer").split().1 as *mut u8;
    let mmap_pages_2 = (system_table.boot_services().memory_map_size() + 4095) / 4096;
    if mmap_pages_2 > mmap_pages {
        panic!("Memory Map unexpectedly grew too much");
    }

    let (_system_table_runtime, _memory_map) = system_table.exit_boot_services(img_handle, unsafe{slice::from_raw_parts_mut(mmap_buffer, mmap_pages * 4096)}).expect("Failed to exit boot services").split().1;

    platform::goto_entrypoint(kernel_header, entry_point);

    Status::LOAD_ERROR
}

#[panic_handler]
fn panic_handler(info: &PanicInfo) -> ! {
    let stdout = unsafe { &mut *STDOUT };

    // explicitly ignore Result since panicking in a panic handler seems hysterical
    let _ = write!(stdout, "PANIC: {:?} at {:?}", info.message(), info.location());

    loop {}
}

#[alloc_error_handler]
fn alloc_error_handler(layout: Layout) -> ! {
    panic!("Failed to allocate {} bytes", layout.size());
}
