use common_structures::PagingInfo;

use crate::arch;

static mut HIGH_MEM_BASE: u64 = 0;

pub fn set_high_mem_base(high_mem_base: u64) {
    unsafe {
        HIGH_MEM_BASE = high_mem_base;
    }
}

pub fn phys_to_virt<T>(phys: u64) -> *mut T {
    unsafe {
        (phys | HIGH_MEM_BASE) as *mut T
    }
}

pub fn virt_to_phys<T>(virt: *mut T) -> u64 {
    unsafe {
        (virt as u64) & !(HIGH_MEM_BASE)
    }
}

pub fn init_virt_manager(paging_info: &PagingInfo) {
    info!("VirtManager", "Starting initialization");

    verbose!("VirtManager", "high_mem_base={:#016X}", unsafe{HIGH_MEM_BASE});

    arch::virt_manager::init(paging_info);

    info!("VirtManager", "Initialized");
}
