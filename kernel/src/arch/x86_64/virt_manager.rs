use common_structures::PagingInfo;

use crate::memory::*;


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
