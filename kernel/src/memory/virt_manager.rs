use common_structures::PagingInfo;

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

    platform::init(paging_info);

    info!("VirtManager", "Initialized");
}

#[cfg(target_arch="x86_64")]
mod platform {
    use super::*;
    pub fn init(paging_info: &PagingInfo) {
        let pml4 = paging_info.page_buffer;
        for i in 0..paging_info.pml4_entries {
            unsafe{pml4.offset(i as isize).write(0);}
        }
        verbose!("VirtManager", "PML4 at phys address {:#016X}", virt_to_phys(pml4));

        let cr3 = virt_to_phys(paging_info.page_buffer);
        unsafe{asm!(
            "mov cr3, {}",
            in(reg) cr3
        )};
    }
}
