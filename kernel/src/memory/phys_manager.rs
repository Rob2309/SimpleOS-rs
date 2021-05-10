use core::{mem::MaybeUninit, slice, ptr::null_mut};
use core::cell::UnsafeCell;

use common_structures::{KernelHeader, MemorySegment, MemorySegmentState};

use crate::mutex::{Lock, SpinLock};

const MAX_ORDER: usize = 8;

pub trait PhysManagerStorage {
    fn new(num_pages: u64, memory_map: &mut [MemorySegment]) -> Self;
    fn get_buddy_map(&mut self) -> &mut [u64];
    fn get_entry(&mut self, index: u64) -> *mut FreeEntry;
}

pub struct PhysMemoryManager<Storage: PhysManagerStorage = InlineStorage> {
    lock: SpinLock,
    free_lists: UnsafeCell<[*mut FreeEntry; MAX_ORDER+1]>,
    storage: UnsafeCell<Storage>,
}

struct FreeEntry {
    order: usize,
    next: *mut FreeEntry,
    prev: *mut FreeEntry,
}

struct InlineStorage {
    buddy_map: *mut [u64],
}

impl PhysManagerStorage for InlineStorage {
    fn new(num_pages: u64, memory_map: &mut [MemorySegment]) -> Self {
        let num_entries = (num_pages + 63) / 64;
        let num_storage_pages = (num_entries * 8 + 4095) / 4096;

        let buddy_map = {
            let entry = memory_map.iter()
                .find(|&entry| entry.state == MemorySegmentState::Free && entry.page_count >= num_storage_pages)
                .expect("No suitable memory location found for buddy map");
            
            let res = entry.start;
            entry.start += num_storage_pages * 4096;
            entry.page_count -= num_storage_pages;
            
            unsafe { slice::from_raw_parts_mut(res as *mut u64, num_entries as usize) as *mut [u64] }
        };

        unsafe {
            (*buddy_map).fill(0);
        }

        Self {
            buddy_map,
        }
    }

    fn get_buddy_map(&mut self) -> &mut [u64] {
        unsafe { &mut *self.buddy_map }
    }

    fn get_entry(&mut self, index: u64) -> *mut FreeEntry {
        (index << 12) as *mut FreeEntry
    }
}

static mut INSTANCE: MaybeUninit<PhysMemoryManager> = MaybeUninit::uninit();

pub fn init(kernel_header: &KernelHeader) {
    unsafe {
        INSTANCE.write(PhysMemoryManager::new(slice::from_raw_parts_mut(kernel_header.memory_map, kernel_header.memory_map_entries as usize)));
    }
}

pub fn phys_manager() -> &'static PhysMemoryManager {
    unsafe {
        &*INSTANCE.as_mut_ptr()
    }
}

unsafe impl<Storage: PhysManagerStorage> Sync for PhysMemoryManager<Storage> {}
unsafe impl<Storage: PhysManagerStorage> Send for PhysMemoryManager<Storage> {}

impl<Storage: PhysManagerStorage> PhysMemoryManager<Storage> {
    pub fn new(memory_map: &mut [MemorySegment]) -> Self {
        let max_address = memory_map.iter()
            .map(|entry| entry.start + entry.page_count * 4096)
            .max().expect("Memory Map is empty");

        let storage = Storage::new(max_address >> 12, memory_map).into();

        Self {
            lock: SpinLock::new(),
            free_lists: [null_mut(); MAX_ORDER+1].into(),
            storage,
        }
    }

    fn free_buddy(buddy_map: &mut [u64], free_lists: &mut [*mut FreeEntry], index: u64, order: u32) {

    }

    fn alloc_buddy(buddy_map: &mut [u64], free_lists: &mut [*mut FreeEntry], order: u32) -> u64 {
        0
    }

    pub fn free_page(&self, addr: u64) {
        let guard = self.lock.lock();
        let buddy_map = self.storage.get_mut().get_buddy_map();
        let free_lists = self.free_lists.get_mut();

        Self::free_buddy(buddy_map, free_lists, addr >> 12, 0);
    }

    pub fn alloc_page(&self) -> u64 {
        let guard = self.lock.lock();
        let buddy_map = self.storage.get_mut().get_buddy_map();
        let free_lists = self.free_lists.get_mut();

        Self::alloc_buddy(buddy_map, free_lists, 0)
    }
}

pub mod api {
    pub use super::phys_manager;
    pub use super::init as init_phys_manager;
}
