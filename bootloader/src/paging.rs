use core::slice;

use uefi::table::{Boot, SystemTable, boot::{AllocateType, MemoryType}};

use core::fmt::Write;

#[cfg(target_arch="x86_64")]
mod platform {
    use super::*;

    /// Present bit of a page table entry. 
    /// If this bit is not set, accessing this page 
    /// will fire a page fault.
    const PML_P: u64 = 0x1;
    /// Writable bit of a page table entry. 
    /// If this bit is set, writing to the given 
    /// page is allowed.
    const PML_RW: u64 = 0x2;

    /// Mask for the physical address field in a PML4 entry.
    const PML4_ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;
    /// Our PML4 entries should be present and writable.
    const PML4_ENTRY_BASE: u64 = PML_P | PML_RW;

    /// Mask for the physical address field in a PDP table entry.
    const PDPE_ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;
    /// Our PDP entries should be present and writable.
    const PDPE_ENTRY_BASE: u64 = PML_P | PML_RW;

    /// Mask for the physical address field in a Page Directory table entry.
    const PDE_ADDR_MASK: u64 = 0x000F_FFFF_FFE0_0000;
    /// Our Page Directory entries should be present and writable. 
    /// Bit 0x80 signals the processor that we use 2MB pages instead of 4KB pages.
    const PDE_ENTRY_BASE: u64 = PML_P | PML_RW | 0x80;

    /// This variable will hold the first memory address in the higher memory half.
    static mut HIGH_MEM_BASE: u64 = 0;

    /// Initializes a page table that contains an identity mapping of physical memory
    /// in the lower memory half (0x0000000000000000 - 0x00007FFFFFFFFFFF) as well as the same mapping in the
    /// higher memory half (0xFFFFXXXXXXXXXXXX - 0xFFFFFFFFFFFFFFFF). 
    pub fn init(system_table: &SystemTable<Boot>, mut physical_size: u64) {
        write!(system_table.stdout(), "Memory ranges from 0 to {:016X}\r\n", physical_size).unwrap();

        /*
            The x86_64 page table is split up into multiple levels of tables.
            Each table entry points to 512 table entries of the next level.
            Since a table entry at every level is 8 bytes, every table takes up exactly one 4096 byte page.
            The structure is:
                Page Map Level 4
                    V
                Page Directory Pointer Table
                    V
                Page Directory Table
                    V
                Page Table
            
                For more info see the AMD64 Architecture Programmer's Manual, Volume 2, Chapter 5 (especially 5.3).
        */

        // Cut of bits 63-47 to ensure that physical memory only occupies half of virtual memory, which is 48 bits wide.
        // On current x86_64 chips, physical memory can theoretically be 52 bits, which does not fit into virtual memory.
        physical_size &= 0x0000_7FFF_FFFF_FFFF;

        // Calculate how many page table entries of each type are needed.
        let pml4_entries = (physical_size >> 39) + 1;
        let pdp_entries = (physical_size >> 30) + 1;
        let pd_entries = (physical_size >> 21) + 1;

        // Calculate how many memory pages are needed for every entry type.
        let pml4_pages = (pml4_entries * 8 + 4095) / 4096;
        let pdp_pages = (pdp_entries * 8 + 4095) / 4096;
        let pd_pages = (pd_entries * 8 + 4095) / 4096;
        let alloc_pages =  pml4_pages + pdp_pages + pd_pages;

        // Since AMD64 spec currently only supports 48 bits of virtual address space, the PML4 table can
        // only contain 512 entries / one memory page.
        assert!(pml4_pages == 1, "PML4 larger than one page, should be impossible");

        write!(system_table.stdout(), "Using {} physical pages for initial page table (pml4_pages={}, pdp_pages={}, pd_pages={})\r\n", alloc_pages, pml4_pages, pdp_pages, pd_pages).unwrap();

        // Allocate storage for the page table.
        let page_buffer_ptr = system_table.boot_services().allocate_pages(AllocateType::AnyPages, MemoryType::LOADER_DATA, alloc_pages as usize).expect("Failed to allocate buffer for page table").split().1 as *mut u64;
        let page_buffer = unsafe{slice::from_raw_parts_mut(page_buffer_ptr, alloc_pages as usize * 4096)};

        // Fill out the Page Map Level 4 (PML4) entries.
        for pml4_entry in 0..pml4_entries {
            let entry_addr = pml4_entry * 4096 + pml4_pages * 4096 + page_buffer_ptr as u64;
            assert!((entry_addr & PML4_ADDR_MASK) == entry_addr, "PML4 Address field misaligned");

            let entry = entry_addr | PML4_ENTRY_BASE;

            // Since we want to mirror physical memory into the higher memory half
            // without using double the storage for the page table,
            // we can just put the same PML4 entries into the higher half entries.
            page_buffer[pml4_entry as usize] = entry;
            page_buffer[512 - pml4_entries as usize + pml4_entry as usize] = entry;
        }

        // Fill out the Page Directory Pointer Table (PDPT) entries.
        for pdp_entry in 0..pdp_entries {
            let entry_addr = pdp_entry * 4096 + pml4_pages * 4096 + pdp_pages * 4096 + page_buffer_ptr as u64;
            assert!((entry_addr & PDPE_ADDR_MASK) == entry_addr, "PDP Address field misaligned");

            let entry = entry_addr | PDPE_ENTRY_BASE;
            page_buffer[pml4_pages as usize * 512 + pdp_entry as usize] = entry;
        }

        // Fill out the Page Directory Table (PDT) entries.
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

        // The CR3 register holds the physical address of the PML4 Table.
        // When written to, all TLB entries are invalidated automatically.
        unsafe{asm!(
            "mov cr3, {}",
            in(reg) page_buffer_ptr
        )};
    }
    
    /// Converts a pointer from the lower memory half to
    /// the higher memory half (i.e. the "kernel memory space")
    pub fn ptr_to_kernelspace<T>(ptr: *mut T) -> *mut T {
        (ptr as u64 | unsafe{HIGH_MEM_BASE}) as *mut T
    }

}

pub use platform::ptr_to_kernelspace;

/// Initializes the platform dependent paging mechanism.
/// See [`platform::init()`] for more info.
pub fn init(system_table: &SystemTable<Boot>) {
    // retrieve the UEFI memory map.
    let mmap_pages = (system_table.boot_services().memory_map_size() + 4095) / 4096 + 1;
    let mmap_buffer = system_table.boot_services().allocate_pages(AllocateType::AnyPages, MemoryType::LOADER_DATA, mmap_pages).expect("Failed to allocate space for memory map").split().1 as *mut u8;
    let mmap_pages_2 = (system_table.boot_services().memory_map_size() + 4095) / 4096;
    assert!(mmap_pages >= mmap_pages_2, "MemoryMap unexpectedly expanded too much");

    let (_mmap_key, mmap) = system_table.boot_services().memory_map(unsafe{slice::from_raw_parts_mut(mmap_buffer, mmap_pages * 4096)}).expect("Failed to retrieve memory map").split().1;

    // iterate through all memory map entries and
    // find the highest physical memory address.
    let mut physical_size = 0u64;
    for e in mmap {
        let end = e.phys_start + e.page_count * 4096;
        if end > physical_size {
            physical_size = end;
        }
    }

    // call the platform dependent init function.
    platform::init(system_table, physical_size);

    // free the memory map buffer.
    let _ = system_table.boot_services().free_pages(mmap_buffer as u64, mmap_pages).expect("Failed to free memory map buffer");
}
