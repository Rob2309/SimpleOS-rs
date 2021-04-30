use core::slice;

use uefi::table::{Boot, SystemTable, boot::{AllocateType, MemoryType}};

use core::fmt::Write;

#[cfg(target_arch="x86_64")]
mod platform {
    use super::*;

    const PML_P: u64 = 0x1;
    const PML_RW: u64 = 0x2;

    const PML4_ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;
    const PML4_ENTRY_BASE: u64 = PML_P | PML_RW;

    const PDPE_ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;
    const PDPE_ENTRY_BASE: u64 = PML_P | PML_RW;

    const PDE_ADDR_MASK: u64 = 0x000F_FFFF_FFE0_0000;
    const PDE_ENTRY_BASE: u64 = PML_P | PML_RW | 0x80;

    static mut HIGH_MEM_BASE: u64 = 0;

    pub fn init(system_table: &SystemTable<Boot>, mut physical_size: u64) {
        write!(system_table.stdout(), "Memory ranges from 0 to {:016X}\r\n", physical_size).unwrap();

        // Cut of bits 63-47 to ensure that physical memory only occupies half of virtual memory, which is 48 bits wide.
        // This limitation is due to the bootloader identity mapping physical memory to both the lower and higher half.
        physical_size &= 0x0000_7FFF_FFFF_FFFF;

        let pml4_entries = (physical_size >> 39) + 1;
        let pdp_entries = (physical_size >> 30) + 1;
        let pd_entries = (physical_size >> 21) + 1;

        let pml4_pages = (pml4_entries * 8 + 4095) / 4096;
        let pdp_pages = (pdp_entries * 8 + 4095) / 4096;
        let pd_pages = (pd_entries * 8 + 4095) / 4096;
        let alloc_pages =  pml4_pages + pdp_pages + pd_pages;

        assert!(pml4_pages == 1, "PML4 larger than one page, should be impossible");

        write!(system_table.stdout(), "Using {} physical pages for initial page table (pml4_pages={}, pdp_pages={}, pd_pages={})\r\n", alloc_pages, pml4_pages, pdp_pages, pd_pages).unwrap();

        let page_buffer_ptr = system_table.boot_services().allocate_pages(AllocateType::AnyPages, MemoryType::LOADER_DATA, alloc_pages as usize).expect("Failed to allocate buffer for page table").split().1 as *mut u64;
        let page_buffer = unsafe{slice::from_raw_parts_mut(page_buffer_ptr, alloc_pages as usize * 4096)};

        for pml4_entry in 0..pml4_entries {
            let entry_addr = pml4_entry * 4096 + pml4_pages * 4096 + page_buffer_ptr as u64;
            assert!((entry_addr & PML4_ADDR_MASK) == entry_addr, "PML4 Address field misaligned");

            let entry = entry_addr | PML4_ENTRY_BASE;
            page_buffer[pml4_entry as usize] = entry;
            page_buffer[512 - pml4_entries as usize + pml4_entry as usize] = entry;
        }

        for pdp_entry in 0..pdp_entries {
            let entry_addr = pdp_entry * 4096 + pml4_pages * 4096 + pdp_pages * 4096 + page_buffer_ptr as u64;
            assert!((entry_addr & PDPE_ADDR_MASK) == entry_addr, "PDP Address field misaligned");

            let entry = entry_addr | PDPE_ENTRY_BASE;
            page_buffer[pml4_pages as usize * 512 + pdp_entry as usize] = entry;
        }

        for pd_entry in 0..pd_entries {
            let entry_addr = pd_entry << 21;
            assert!((entry_addr & PDE_ADDR_MASK) == entry_addr, "PD Address field misaligned");

            let entry = entry_addr | PDE_ENTRY_BASE;
            page_buffer[pml4_pages as usize * 512 + pdp_pages as usize * 512 + pd_entry as usize] = entry;
        }

        unsafe {
            HIGH_MEM_BASE = 0xFFFF_0000_0000_0000 | ((512 - pml4_entries) << 39);
            write!(system_table.stdout(), "High memory start: {:#016X}\r\n", HIGH_MEM_BASE).unwrap();
        }

        unsafe{asm!(
            "mov cr3, {}",
            in(reg) page_buffer_ptr
        )};
    }
    
    pub fn ptr_to_kernelspace<T>(ptr: *mut T) -> *mut T {
        (ptr as u64 | unsafe{HIGH_MEM_BASE}) as *mut T
    }

}

pub use platform::ptr_to_kernelspace;

pub fn init(system_table: &SystemTable<Boot>) {
    let mmap_pages = (system_table.boot_services().memory_map_size() + 4095) / 4096 + 1;
    let mmap_buffer = system_table.boot_services().allocate_pages(AllocateType::AnyPages, MemoryType::LOADER_DATA, mmap_pages).expect("Failed to allocate space for memory map").split().1 as *mut u8;
    let mmap_pages_2 = (system_table.boot_services().memory_map_size() + 4095) / 4096;
    assert!(mmap_pages >= mmap_pages_2, "MemoryMap unexpectedly expanded too much");

    let (_mmap_key, mmap) = system_table.boot_services().memory_map(unsafe{slice::from_raw_parts_mut(mmap_buffer, mmap_pages * 4096)}).expect("Failed to retrieve memory map").split().1;

    let mut physical_size = 0u64;
    for e in mmap {
        let end = e.phys_start + e.page_count * 4096;
        if end > physical_size {
            physical_size = end;
        }
    }

    platform::init(system_table, physical_size);

    let _ = system_table.boot_services().free_pages(mmap_buffer as u64, mmap_pages).expect("Failed to free memory map buffer");
}
