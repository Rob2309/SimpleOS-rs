use core::{mem::MaybeUninit, slice, ptr::null_mut};
use core::cell::UnsafeCell;

use common_structures::{KernelHeader, MemorySegment, MemorySegmentState};

use crate::mutex::{Lock, SpinLock};

const MAX_ORDER: usize = 8;

pub trait PhysManagerStorage {
    fn new(num_pages: u64, memory_map: &mut [MemorySegment]) -> Self;
    fn get_buddy_map(&mut self) -> &mut [u64];
    fn get_entry(&mut self, index: u64) -> *mut FreeEntry;
    fn get_index(&mut self, entry: *mut FreeEntry) -> u64;
}

pub struct PhysMemoryManager<Storage: PhysManagerStorage = InlineStorage> {
    lock: SpinLock,
    free_lists: UnsafeCell<[*mut FreeEntry; MAX_ORDER+1]>,
    storage: UnsafeCell<Storage>,
}

pub struct FreeEntry {
    order: usize,
    next: *mut FreeEntry,
    prev: *mut FreeEntry,
}

pub struct InlineStorage {
    buddy_map: *mut [u64],
}

impl PhysManagerStorage for InlineStorage {
    fn new(num_pages: u64, memory_map: &mut [MemorySegment]) -> Self {
        let num_entries = (num_pages + 63) / 64;
        let num_storage_pages = (num_entries * 8 + 4095) / 4096;

        let buddy_map = {
            let entry = memory_map.iter_mut()
                .find(|entry| entry.state == MemorySegmentState::Free && entry.page_count >= num_storage_pages)
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

    fn get_index(&mut self, entry: *mut FreeEntry) -> u64 {
        (entry as u64) >> 12
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

        let res = Self {
            lock: SpinLock::new(),
            free_lists: [null_mut(); MAX_ORDER+1].into(),
            storage,
        };

        for entry in memory_map.iter().filter(|&e| e.state == MemorySegmentState::Free) {
            res.add_region(entry.start >> 12, entry.page_count);
        }

        res
    }

    fn add_region(&self, mut index: u64, mut page_count: u64) {
        let _guard = self.lock.lock();
        let storage = unsafe{&mut *self.storage.get()};
        let free_lists = unsafe{&mut *self.free_lists.get()};

        while page_count > 0 {
            let index_order = index.trailing_zeros();
            let count_order = 63 - page_count.leading_zeros();
            let order = index_order.min(count_order).min(MAX_ORDER as u32);

            Self::free_block(storage, free_lists, index, order);

            index += 1 << order;
            page_count -= 1 << order;
        }
    }

    fn get_buddy_index(index: u64, order: u32) -> u64 {
        index ^ (1 << order)
    }

    fn get_combined_index(index: u64, order: u32) -> u64 {
        index & !(1 << order)
    }

    fn get_size_order(count: u64) -> u32 {
        let order = 63 - count.leading_zeros();
        if count & (1 << order) != count {
            order + 1
        } else {
            order
        }
    }

    fn remove_buddy_list_entry(head: &mut *mut FreeEntry, entry: *mut FreeEntry) {
        unsafe {
            if (*entry).prev == null_mut() {
                *head = (*entry).next;
            } else {
                (*(*entry).prev).next = (*entry).next;
            }
            if (*entry).next != null_mut() {
                (*(*entry).next).prev = (*entry).prev;
            }
        }
    }

    fn push_buddy_list_entry(head: &mut *mut FreeEntry, entry: *mut FreeEntry) {
        unsafe {
            if *head != null_mut() {
                (*entry).next = *head;
                (**head).prev = entry;
            }
            *head = entry;
        }
    }

    fn pop_buddy_list_entry(head: &mut *mut FreeEntry) -> *mut FreeEntry {
        unsafe {
            if *head == null_mut() {
                null_mut()
            } else {
                let tmp = *head;
                *head = (**head).next;
                if *head != null_mut() {
                    (**head).prev = null_mut();
                }
                tmp
            }
        }
    }

    fn free_block(storage: &mut Storage, free_lists: &mut [*mut FreeEntry], index: u64, order: u32) {
        let entry = index / 64;
        let bit = index % 64;
        let entry_ptr = storage.get_entry(index);

        let buddy_index = Self::get_buddy_index(index, order);
        let buddy_entry = buddy_index / 64;
        let buddy_bit = buddy_index % 64;
        let buddy_ptr = storage.get_entry(buddy_index);

        let buddy_map = storage.get_buddy_map();

        if order < MAX_ORDER as u32 && buddy_map[buddy_entry as usize] & (1 << buddy_bit) != 0 && unsafe{ (*buddy_ptr).order == order as usize } {
            buddy_map[buddy_entry as usize] &= !(1 << buddy_bit);
            Self::remove_buddy_list_entry(&mut free_lists[order as usize], buddy_ptr);
            Self::free_block(storage, free_lists, Self::get_combined_index(index, order), order+1);
        } else {
            buddy_map[entry as usize] |= 1 << bit;
            unsafe{entry_ptr.write(FreeEntry {
                order: order as usize,
                next: null_mut(),
                prev: null_mut(),
            })};
            Self::push_buddy_list_entry(&mut free_lists[order as usize], entry_ptr);
        }
    }

    fn alloc_block(storage: &mut Storage, free_lists: &mut [*mut FreeEntry], order: u32) -> u64 {
        let entry = Self::pop_buddy_list_entry(&mut free_lists[order as usize]);

        if entry == null_mut() {
            if (order as usize) == MAX_ORDER {
                panic!("Out of physical memory");
            }

            let higher_block = Self::alloc_block(storage, free_lists, order+1);
            let buddy_index = Self::get_buddy_index(higher_block, order);
            let buddy_entry = buddy_index / 64;
            let buddy_bit = buddy_index % 64;
            let buddy_ptr = storage.get_entry(buddy_index);

            let buddy_map = storage.get_buddy_map();

            buddy_map[buddy_entry as usize] |= 1 << buddy_bit;

            unsafe{buddy_ptr.write(FreeEntry {
                order: order as usize,
                next: null_mut(),
                prev: null_mut(),
            })};
            Self::push_buddy_list_entry(&mut free_lists[order as usize], buddy_ptr);

            higher_block
        } else {
            let index = storage.get_index(entry);
            let entry = index / 64;
            let bit = index % 64;

            let buddy_map = storage.get_buddy_map();

            buddy_map[entry as usize] &= !(1 << bit);
            index
        }
    }

    pub fn free_page(&self, addr: u64) {
        let _guard = self.lock.lock();
        let storage = unsafe{&mut *self.storage.get()};
        let free_lists = unsafe{&mut *self.free_lists.get()};

        Self::free_block(storage, free_lists, addr >> 12, 0);
    }

    pub fn free_linear_pages(&self, addr: u64, count: u64) {
        let _guard = self.lock.lock();
        let storage = unsafe{&mut *self.storage.get()};
        let free_lists = unsafe{&mut *self.free_lists.get()};

        Self::free_block(storage, free_lists, addr >> 12, Self::get_size_order(count));
    }

    pub fn free_pages(&self, addresses: &[u64]) {
        let _guard = self.lock.lock();
        let storage = unsafe{&mut *self.storage.get()};
        let free_lists = unsafe{&mut *self.free_lists.get()};

        for addr in addresses {
            Self::free_block(storage, free_lists, addr >> 12, 0);
        }
    }

    pub fn alloc_page(&self) -> u64 {
        let _guard = self.lock.lock();
        let storage = unsafe{&mut *self.storage.get()};
        let free_lists = unsafe{&mut *self.free_lists.get()};

        Self::alloc_block(storage, free_lists, 0) << 12
    }

    pub fn alloc_linear_pages(&self, count: u64) -> u64 {
        let _guard = self.lock.lock();
        let storage = unsafe{&mut *self.storage.get()};
        let free_lists = unsafe{&mut *self.free_lists.get()};

        Self::alloc_block(storage, free_lists, Self::get_size_order(count)) << 12
    }

    pub fn alloc_pages(&self, addresses: &mut [u64]) {
        let _guard = self.lock.lock();
        let storage = unsafe{&mut *self.storage.get()};
        let free_lists = unsafe{&mut *self.free_lists.get()};

        for out_addr in addresses {
            *out_addr = Self::alloc_block(storage, free_lists, 0) << 12;
        }
    }
}

pub mod api {
    pub use super::phys_manager;
    pub use super::init as init_phys_manager;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestStorage {
        buddy_map: Vec<u64>,
        memory: Vec<u8>,
    }

    impl PhysManagerStorage for TestStorage {
        fn new(num_pages: u64, _memory_map: &mut [MemorySegment]) -> Self {
            let num_entries = (num_pages + 63) / 64;

            let buddy_map = vec![0; num_entries as usize];
            let memory = vec![0; (num_pages * 4096) as usize];

            Self {
                buddy_map,
                memory,
            }
        }

        fn get_buddy_map(&mut self) -> &mut [u64] {
            &mut self.buddy_map
        }

        fn get_entry(&mut self, index: u64) -> *mut FreeEntry {
            (self.memory.as_ptr() as u64 + (index << 12)) as *mut FreeEntry
        }

        fn get_index(&mut self, entry: *mut FreeEntry) -> u64 {
            (entry as u64 - self.memory.as_ptr() as u64) >> 12
        }
    }

    #[test]
    fn count_to_order() {
        assert!(PhysMemoryManager::<TestStorage>::get_size_order(1) == 0);
        assert!(PhysMemoryManager::<TestStorage>::get_size_order(2) == 1);
        assert!(PhysMemoryManager::<TestStorage>::get_size_order(3) == 2);
        assert!(PhysMemoryManager::<TestStorage>::get_size_order(4) == 2);

        assert!(PhysMemoryManager::<TestStorage>::get_size_order(13) == 4);
    }

    #[test]
    fn free_single() {
        let mmap = &mut [
            MemorySegment {
                start: 0,
                page_count: 30,
                state: MemorySegmentState::Occupied,
            },
        ];

        let mut manager = PhysMemoryManager::<TestStorage>::new(mmap);

        manager.free_page(7 * 4096);
        
        unsafe {
            assert!(manager.storage.get_mut().get_buddy_map()[0] & (1 << 7) != 0);

            assert!(manager.free_lists.get_mut()[0] != null_mut());
            assert!((*manager.free_lists.get_mut()[0]).next == null_mut());
            assert!((*manager.free_lists.get_mut()[0]).prev == null_mut());
            assert!((*manager.free_lists.get_mut()[0]).order == 0);
        }
    }

    #[test]
    fn free_merge_forward() {
        let mmap = &mut [
            MemorySegment {
                start: 0,
                page_count: 30,
                state: MemorySegmentState::Occupied,
            },
        ];

        let mut manager = PhysMemoryManager::<TestStorage>::new(mmap);

        manager.free_page(6 * 4096);
        manager.free_page(7 * 4096);

        unsafe {
            assert!(manager.storage.get_mut().get_buddy_map()[0] & (1 << 6) != 0);
            assert!(manager.storage.get_mut().get_buddy_map()[0] & (1 << 7) == 0);

            assert!(manager.free_lists.get_mut()[0] == null_mut());

            assert!(manager.free_lists.get_mut()[1] != null_mut());
            assert!((*manager.free_lists.get_mut()[1]).next == null_mut());
            assert!((*manager.free_lists.get_mut()[1]).prev == null_mut());
            assert!((*manager.free_lists.get_mut()[1]).order == 1);
        }
    }

    #[test]
    fn free_merge_backward() {
        let mmap = &mut [
            MemorySegment {
                start: 0,
                page_count: 30,
                state: MemorySegmentState::Occupied,
            },
        ];

        let mut manager = PhysMemoryManager::<TestStorage>::new(mmap);

        manager.free_page(7 * 4096);
        manager.free_page(6 * 4096);

        unsafe {
            assert!(manager.storage.get_mut().get_buddy_map()[0] & (1 << 6) != 0);
            assert!(manager.storage.get_mut().get_buddy_map()[0] & (1 << 7) == 0);

            assert!(manager.free_lists.get_mut()[0] == null_mut());

            assert!(manager.free_lists.get_mut()[1] != null_mut());
            assert!((*manager.free_lists.get_mut()[1]).next == null_mut());
            assert!((*manager.free_lists.get_mut()[1]).prev == null_mut());
            assert!((*manager.free_lists.get_mut()[1]).order == 1);
        }
    }

    #[test]
    fn free_dont_merge_different_orders() {
        let mmap = &mut [
            MemorySegment {
                start: 0,
                page_count: 1,
                state: MemorySegmentState::Free,
            },
            MemorySegment {
                start: 1,
                page_count: 3,
                state: MemorySegmentState::Occupied,
            }
        ];

        let mut manager = PhysMemoryManager::<TestStorage>::new(mmap);

        manager.free_linear_pages(2 * 4096, 2);

        unsafe {
            assert!(manager.storage.get_mut().get_buddy_map()[0] & (1 << 0) != 0);
            assert!(manager.storage.get_mut().get_buddy_map()[0] & (1 << 1) == 0);
            assert!(manager.storage.get_mut().get_buddy_map()[0] & (1 << 2) != 0);
            assert!(manager.storage.get_mut().get_buddy_map()[0] & (1 << 3) == 0);

            assert!(manager.free_lists.get_mut()[0] != null_mut());
            assert!((*manager.free_lists.get_mut()[0]).next == null_mut());
            assert!((*manager.free_lists.get_mut()[0]).prev == null_mut());
            assert!((*manager.free_lists.get_mut()[0]).order == 0);

            assert!(manager.free_lists.get_mut()[1] != null_mut());
            assert!((*manager.free_lists.get_mut()[1]).next == null_mut());
            assert!((*manager.free_lists.get_mut()[1]).prev == null_mut());
            assert!((*manager.free_lists.get_mut()[1]).order == 1);
        }
    }

    #[test]
    fn init_dont_merge_max_order() {
        let mmap = &mut [
            MemorySegment {
                start: 0,
                page_count: (1 << MAX_ORDER) * 2,
                state: MemorySegmentState::Free,
            },
        ];

        let mut manager = PhysMemoryManager::<TestStorage>::new(mmap);

        let index = 1 << MAX_ORDER;
        let entry = index / 64;
        let bit = index % 64;

        unsafe {
            assert!(manager.storage.get_mut().get_buddy_map()[0] & (1 << 0) != 0);
            assert!(manager.storage.get_mut().get_buddy_map()[entry as usize] & (1 << bit) != 0);

            assert!(manager.free_lists.get_mut()[MAX_ORDER] != null_mut());
            assert!((*manager.free_lists.get_mut()[MAX_ORDER]).next != null_mut());
            assert!((*manager.free_lists.get_mut()[MAX_ORDER]).prev == null_mut());
            assert!((*manager.free_lists.get_mut()[MAX_ORDER]).order == MAX_ORDER);
        }
    }

    #[test]
    fn alloc_single() {
        let mmap = &mut [
            MemorySegment {
                start: 0,
                page_count: 1,
                state: MemorySegmentState::Free,
            },
        ];

        let mut manager = PhysMemoryManager::<TestStorage>::new(mmap);

        let page = manager.alloc_page();
        assert!(page == 0);

        assert!(manager.storage.get_mut().get_buddy_map()[0] & (1 << 0) == 0);
        assert!(manager.free_lists.get_mut()[0] == null_mut());
    }

    #[test]
    fn alloc_split() {
        let mmap = &mut [
            MemorySegment {
                start: 0,
                page_count: 2,
                state: MemorySegmentState::Free,
            },
        ];

        let mut manager = PhysMemoryManager::<TestStorage>::new(mmap);

        let page = manager.alloc_page();
        assert!(page == 0);

        assert!(manager.storage.get_mut().get_buddy_map()[0] & (1 << 0) == 0);
        assert!(manager.storage.get_mut().get_buddy_map()[0] & (1 << 1) != 0);
        assert!(manager.free_lists.get_mut()[0] != null_mut());
        assert!(manager.free_lists.get_mut()[1] == null_mut());
    }

    #[test]
    fn init_free_regions() {
        {
            let mmap = &mut [
                MemorySegment {
                    start: 68 * 4096,
                    page_count: 2,
                    state: MemorySegmentState::Free,
                },
                MemorySegment {
                    start: 70 * 4096,
                    page_count: 2,
                    state: MemorySegmentState::Free,
                },
            ];

            let mut manager = PhysMemoryManager::<TestStorage>::new(mmap);

            unsafe {
                assert!(manager.storage.get_mut().get_buddy_map()[1] & (1 << 4) != 0);
                assert!(manager.storage.get_mut().get_buddy_map()[1] & (1 << 6) == 0);

                assert!(manager.free_lists.get_mut()[0] == null_mut());
                assert!(manager.free_lists.get_mut()[1] == null_mut());
                assert!(manager.free_lists.get_mut()[2] != null_mut());

                assert!((*manager.free_lists.get_mut()[2]).next == null_mut());
                assert!((*manager.free_lists.get_mut()[2]).prev == null_mut());
                assert!((*manager.free_lists.get_mut()[2]).order == 2);
            }
        }
        {
            let mmap = &mut [
                MemorySegment {
                    start: 68 * 4096,
                    page_count: 2,
                    state: MemorySegmentState::Free,
                },
                MemorySegment {
                    start: 70 * 4096,
                    page_count: 1,
                    state: MemorySegmentState::Free,
                },
            ];

            let mut manager = PhysMemoryManager::<TestStorage>::new(mmap);

            unsafe {
                assert!(manager.storage.get_mut().get_buddy_map()[1] & (1 << 4) != 0);
                assert!(manager.storage.get_mut().get_buddy_map()[1] & (1 << 6) != 0);

                assert!(manager.free_lists.get_mut()[0] != null_mut());
                assert!(manager.free_lists.get_mut()[1] != null_mut());
                assert!(manager.free_lists.get_mut()[2] == null_mut());

                assert!((*manager.free_lists.get_mut()[0]).next == null_mut());
                assert!((*manager.free_lists.get_mut()[0]).prev == null_mut());
                assert!((*manager.free_lists.get_mut()[0]).order == 0);

                assert!((*manager.free_lists.get_mut()[1]).next == null_mut());
                assert!((*manager.free_lists.get_mut()[1]).prev == null_mut());
                assert!((*manager.free_lists.get_mut()[1]).order == 1);
            }
        }
    }

}
