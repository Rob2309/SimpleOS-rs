#![no_std]
#![no_main]
#![feature(abi_efiapi)]
#![feature(option_result_unwrap_unchecked)]
#![feature(panic_info_message)]
#![feature(alloc_error_handler)]
#![feature(asm)]

use core::{panic::PanicInfo, slice};

use uefi::{prelude::*, proto::{console::{gop::{GraphicsOutput, PixelFormat}, text::Output}, loaded_image::LoadedImage, media::fs::SimpleFileSystem}, table::boot::{AllocateType, MemoryType}};
use core::fmt::Write;

mod allocator;
mod io;
mod elf;
mod paging;
mod platform;

use common_structures::{KernelHeader, MemorySegment, MemorySegmentState, config};

/// Used by the [panic_handler()] to print error messages
static mut STDOUT: *mut Output = core::ptr::null_mut();
/// Used by the [io] module to read files from the boot filesystem
static mut FILESYSTEM: *mut SimpleFileSystem = core::ptr::null_mut();

/// The UEFI Application entry point. Will be called directly by the system firmware
#[no_mangle]
extern "efiapi" fn efi_main(img_handle: Handle, system_table: SystemTable<Boot>) -> Status {
    unsafe {
        STDOUT = system_table.stdout() as *const _ as *mut _;
    }

    // clear the screen
    let _ = system_table.stdout().reset(true);
    write!(system_table.stdout(), "Initializing bootloader...\r\n").unwrap();

    // The LoadedImage Protocol gives information about the currently running UEFI application,
    // including the storage device it is located on
    let loaded_image = system_table.boot_services().handle_protocol::<LoadedImage>(img_handle).expect("LoadedImageProtocol not found").split().1;
    // The SimpleFileSystem Protocol can be used to open files on a storage device.
    // In this case, we want to open the filesystem our UEFI bootloader application is located on,
    // thus we pass loaded_image.device()
    let file_system = system_table.boot_services().handle_protocol::<SimpleFileSystem>(unsafe{&mut *loaded_image.get()}.device()).expect("SimpleFileSystemProtocol not found").split().1;
    // The GraphicsOutput Protocol can be used to obtain a raw framebuffer.
    // This framebuffer can still be used after exiting the UEFI Boot Services (see further below).
    // The framebuffer will be the primary means by which the kernel can print to the screen.
    let graphics = system_table.boot_services().locate_protocol::<GraphicsOutput>().expect("GraphicsOutputProtocol not found").split().1;

    // Save the FileSystem Protocol for use by the io module
    unsafe {
        FILESYSTEM = file_system.get();
    }

    // Allocate storage for the KernelHeader that will be passed to the kernel entry point
    let mut kernel_header = allocator::allocate_object::<KernelHeader>(&system_table, MemoryType::LOADER_DATA);

    write!(system_table.stdout(), "Initializing Paging...\r\n").unwrap();

    // initialize page tables so that the higher memory half mirrors the lower half.
    // Since we want the kernel to be located in the higher memory half, but the UEFI page table
    // will contain only an identity mapping (virtual address == physical address), we have to clone this mapping to the higher memory half.
    paging::init(&system_table, &mut kernel_header.paging_info);

    // convert kernel_header address to the corresponding higher memory half address,
    // so that the kernel can use the header.
    kernel_header = unsafe{&mut *paging::ptr_to_kernelspace(kernel_header)};

    write!(system_table.stdout(), "Switching video mode...\r\n").unwrap();

    // select best video mode and enable it
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

        let m = res_best_mode.expect("No suitable video mode found");
        let _ = gfx.set_mode(&m).expect("Failed to set video mode");

        kernel_header.screen_width = m.info().resolution().0 as u32;
        kernel_header.screen_height = m.info().resolution().1 as u32;
        kernel_header.screen_scanline_width = m.info().stride() as u32;
        kernel_header.screen_buffer = gfx.frame_buffer().as_mut_ptr();
    }

    write!(system_table.stdout(), "Loading modules...\r\n").unwrap();

    // read the raw kernel ELF file from disk
    let kernel_image = io::read_file(&system_table, "EFI\\BOOT\\kernel.sys");
    // find out how much virtual address space the kernel will take after being prepared
    let kernel_elf_size = elf::get_size(kernel_image.data);

    write!(system_table.stdout(), "Kernel size: {}\r\n", kernel_elf_size).unwrap();
    write!(system_table.stdout(), "Preparing kernel...\r\n").unwrap();

    // allocate memory for the prepared kernel image
    let process_buffer = paging::ptr_to_kernelspace(allocator::allocate(&system_table, kernel_elf_size, MemoryType::LOADER_DATA));
    // prepare the kernel and retrieve the kernel entry point
    let entry_point = elf::prepare(kernel_image.data, process_buffer);

    write!(system_table.stdout(), "Kernel at {:#016X} (entry point {:#016X})\r\n", process_buffer as u64, entry_point).unwrap();

    // If we are compiling in debug mode, prepare a single memory page at address 0x1000.
    // The first 8 bytes of this buffer will be read by the debugger and should contain
    // the address of the ".text" section of the prepared kernel image.
    // This address is then used by the debugger to correctly display kernel symbols.
    #[cfg(debug_assertions)]
    {
        let debug_data = system_table.boot_services().allocate_pages(AllocateType::Address(0x1000), MemoryType::LOADER_DATA, 1).expect("Failed to allocate debug buffer").split().1 as *mut u64;
        unsafe {
            *debug_data = elf::get_text_addr(kernel_image.data, process_buffer);
        }
    }

    // free the raw kernel image as we only need the prepared image from now on
    allocator::free(&system_table, kernel_image.data, kernel_image.size as usize);

    // allocate a stack for the kernel
    let kernel_stack = allocator::allocate(&system_table, config::KERNEL_STACK_SIZE as usize, MemoryType::LOADER_DATA);

    write!(system_table.stdout(), "Starting kernel...\r\n").unwrap();

    // Calculate the space needed to retrieve the UEFI memory map.
    // Add one page for safety, as the allocation of the memory map buffer might
    // grow the memory map, resulting in more space being needed to retrieve the memory map.
    let mmap_pages = (system_table.boot_services().memory_map_size() + 4095) / 4096 + 1;
    // Allocate buffer for retrieving the memory map (reserve twice the required size, 
    // we will need the second buffer for converting to kernel_header format).
    let mmap_buffer = system_table.boot_services().allocate_pages(AllocateType::AnyPages, MemoryType::LOADER_DATA, mmap_pages * 2).expect("Failed to allocate mmap buffer").split().1 as *mut u8;
    // ensure that the buffer allocation didn't grow the memory map too much (should never happen)
    let mmap_pages_2 = (system_table.boot_services().memory_map_size() + 4095) / 4096;
    if mmap_pages_2 > mmap_pages {
        panic!("Memory Map unexpectedly grew too much");
    }

    // This call signals to the UEFI firmware that we are finished booting up.
    // exit_boot_services makes the UEFI boot services unavailable, so e.g. memory allocations have to be handled manually.
    // It also stops the so called WatchDog timer, which is around 5 minutes. When this timer runs out before exit_boot_services is called,
    // the firmware will assume that the bootloader is stuck and kill it.
    let (_system_table_runtime, uefi_memory_map) = system_table.exit_boot_services(img_handle, unsafe{slice::from_raw_parts_mut(mmap_buffer, mmap_pages * 4096)}).expect("Failed to exit boot services").split().1;

    // pre-save the memory map entry count, as len() returns the *remaining* entries
    let memory_map_entries = uefi_memory_map.len();
    let memory_map = unsafe{slice::from_raw_parts_mut(mmap_buffer.offset(mmap_pages as isize * 4096) as *mut MemorySegment, memory_map_entries)};

    for (i, entry) in uefi_memory_map.enumerate() {
        memory_map[i] = MemorySegment {
            start: entry.phys_start,
            page_count: entry.page_count,
            state: match entry.ty {
                // after entering the kernel, memory reserved for the bootloader code and uefi boot services are no longer needed.
                MemoryType::BOOT_SERVICES_CODE | 
                MemoryType::BOOT_SERVICES_DATA | 
                MemoryType::CONVENTIONAL | 
                MemoryType::LOADER_CODE => MemorySegmentState::Free,
                _ => MemorySegmentState::Occupied,
            },
        };
    }

    kernel_header.memory_map = memory_map.as_mut_ptr();
    kernel_header.memory_map_entries = memory_map_entries as u64;

    // Jump to the kernel
    platform::goto_entrypoint(kernel_header, entry_point, kernel_stack);
}

/// Will be called by functions like panic!(), expect(), unwrap(), etc. when errors occur.
#[panic_handler]
fn panic_handler(info: &PanicInfo) -> ! {
    let stdout = unsafe { &mut *STDOUT };

    // explicitly ignore Result since panicking in a panic handler seems hysterical
    let _ = write!(stdout, "PANIC: {:?} at {:?}", info.message(), info.location());

    loop {}
}
