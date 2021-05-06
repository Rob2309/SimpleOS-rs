use core::{mem::MaybeUninit, ops::Deref, slice};

use common_structures::{KernelHeader, MemorySegmentState};

use crate::mutex::{Lock, SpinLock};


pub struct PhysMemoryManager {
    page_map: SpinLock<*mut [u64]>,
}

static mut INSTANCE: MaybeUninit<PhysMemoryManager> = MaybeUninit::uninit();

impl PhysMemoryManager {
    pub fn init(kernel_header: &KernelHeader) {
        let max_address = {
            let mut tmp = 0;
            for i in 0..kernel_header.memory_map_entries {
                let entry = unsafe{&*kernel_header.memory_map.offset(i as isize)};
                if entry.start + entry.page_count * 4096 > tmp {
                    tmp = entry.start + entry.page_count * 4096;
                }
            }
            tmp
        };

        let max_page = max_address / 4096;
        let entry_count = (max_page + 63) / 64;
        let page_count = (entry_count + 511) / 512;

        let page_map_addr = {
            let mut tmp = None;
            for i in 0..kernel_header.memory_map_entries {
                let entry = unsafe{&mut *kernel_header.memory_map.offset(i as isize)};
                if entry.state == MemorySegmentState::Free && entry.page_count >= page_count {
                    tmp = Some(entry.start);
                    entry.start += page_count * 4096;
                    entry.page_count -= page_count;
                    break;
                }
            }
            tmp.expect("No suitable location for kernel physical memory map found") as *mut u64
        };

        let page_map = unsafe{slice::from_raw_parts_mut(page_map_addr, entry_count as usize)};

        unsafe {
            INSTANCE = MaybeUninit::new(Self {
                page_map: SpinLock::new(page_map),
            });
        }

        for i in 0..kernel_header.memory_map_entries {
            let entry = unsafe{&mut *kernel_header.memory_map.offset(i as isize)};
            if entry.state == MemorySegmentState::Free {
                for p in 0..entry.page_count {
                    Self::get().free_page(entry.start + p * 4096);
                }
            }
        }
    }

    pub fn get() -> &'static Self {
        unsafe {
            &*INSTANCE.as_ptr()
        }
    }

    pub fn free_page(&self, addr: u64) {
        let page_index = addr / 4096;
        let entry_index = page_index / 64;
        let bit_index = page_index % 64;

        unsafe {
            (&mut **self.page_map.lock())[entry_index as usize] |= 1 << bit_index;
        }
    }

}
