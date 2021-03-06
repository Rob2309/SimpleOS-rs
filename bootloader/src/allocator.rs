use core::{mem::size_of, usize};

use uefi::table::{Boot, SystemTable, boot::{AllocateType, MemoryType}};

/// Allocates a buffer with `size` rounded up to the page size.
pub fn allocate(system_table: &SystemTable<Boot>, size: usize, memory_type: MemoryType) -> *mut u8 {
    let res = system_table.boot_services().allocate_pages(AllocateType::AnyPages, memory_type, (size + 4095) / 4096).expect("Failed to allocate pages").split().1;
    res as *mut u8
}

/// Wrapper around [`allocate()`] for less redundant casting
pub fn allocate_object<T: Sized>(system_table: &SystemTable<Boot>, memory_type: MemoryType) -> &'static mut T {
    unsafe {
        &mut *(allocate(system_table, size_of::<T>(), memory_type) as *mut T)
    }
}

/// Frees memory allocated with [`allocate`] or [`allocate_object`]
pub fn free(system_table: &SystemTable<Boot>, block: *mut u8, size: usize) {
    let _ = system_table.boot_services().free_pages(block as u64, (size + 4095) / 4096).expect("Failed to free pages");
}

/// Allocates memory below the given `max_address`.
/// Primarily useful for processor startup buffers, as x86_64 cores still start up in 16-Bit real mode
/// and thus can only reference memory in the 16-Bit area
#[allow(dead_code)]
pub fn allocate_below(system_table: &SystemTable<Boot>, max_address: usize, num_pages: usize, memory_type: MemoryType) -> Option<*mut u8> {
    let res = system_table.boot_services().allocate_pages(AllocateType::MaxAddress(max_address), memory_type, num_pages);
    res.ok().map(|addr| addr.split().1 as *mut u8)
}
