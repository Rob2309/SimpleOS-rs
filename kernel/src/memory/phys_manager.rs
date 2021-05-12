use core::{mem::MaybeUninit, slice, ptr::null_mut};
use core::cell::UnsafeCell;

use common_structures::{KernelHeader, MemorySegment, MemorySegmentState};

use crate::mutex::{Lock, SpinLock};

use super::{phys_to_virt, virt_to_phys};

/// Maximum order a buddy allocation can have.
/// 
/// 2^8 pages = 256 pages = 1MB
const MAX_ORDER: usize = 8;

/// Interface to tell the [`PhysMemoryManager`] where to place its structures.
/// 
/// Mainly used to allow unit testing of the [`PhysMemoryManager`]. When running the kernel normally,
/// the [`PhysMemoryManager`] will place some of its structures directly in unallocated physical memory.
/// Since this obviously won't work while running in a hosted environment, we need a middleware to
/// alter this behavior when unit testing.
pub trait PhysManagerStorage {
    /// Called by the [`PhysMemoryManager`] to create a new instance of the given Storage backend.
    /// 
    /// This function is allowed to freely modify the given `memory_map`, e.g. if 
    /// physical memory is allocated for the Memory Manager itself.
    fn new(num_pages: u64, memory_map: &mut [MemorySegment]) -> Self;
    /// Should return the bitmap containing the status of every physical memory page.
    fn get_buddy_map(&mut self) -> &mut [u64];
    /// Should return a pointer to the storage of a given buddy entry.
    fn get_entry(&mut self, index: u64) -> *mut FreeEntry;
    /// Should return the index of a given `entry`.
    fn get_index(&mut self, entry: *mut FreeEntry) -> u64;
}

/// Manages allocation and deallocation of physical memory.
pub struct PhysMemoryManager<Storage: PhysManagerStorage = InlineStorage> {
    /// Lock to ensure thread-safe access to all the other fields.
    lock: SpinLock,
    /// Array of linked lists, containing all free areas of a given
    /// size order.
    free_lists: UnsafeCell<[*mut FreeEntry; MAX_ORDER+1]>,
    /// The storage backend object. See [`PhysManagerStorage`].
    storage: UnsafeCell<Storage>,
}

/// Describes an unallocated area of physical memory.
pub struct FreeEntry {
    /// Size order of the memory area.
    order: usize,
    /// The next unallocated area of the same order, if any.
    next: *mut FreeEntry,
    /// The previous unallocated area of the same order, if any.
    prev: *mut FreeEntry,
}

/// Default implementation of [`PhysManagerStorage`].
/// 
/// This implementation will allocate a block of memory for the buddy bitmap
/// and place every [`FreeEntry`] directly into the unallocated memory area it describes.
pub struct InlineStorage {
    buddy_map: *mut [u64],
}

impl PhysManagerStorage for InlineStorage {
    fn new(num_pages: u64, memory_map: &mut [MemorySegment]) -> Self {
        let num_entries = (num_pages + 63) / 64;
        let num_storage_pages = (num_entries * 8 + 4095) / 4096;

        let buddy_map = {
            // find a suitable MemorySegment that is large enough and marked as free
            let entry = memory_map.iter_mut()
                .find(|entry| entry.state == MemorySegmentState::Free && entry.page_count >= num_storage_pages)
                .expect("No suitable memory location found for buddy map");
            
            let res = phys_to_virt::<u8>(entry.start);

            // mark the space for the buddy bitmap as occupied by reducing the size
            // of the selected MemorySegment.
            entry.start += num_storage_pages * 4096;
            entry.page_count -= num_storage_pages;
            
            unsafe { slice::from_raw_parts_mut(res as *mut u64, num_entries as usize) as *mut [u64] }
        };

        // mark every page as occupied.
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
        // By multiplying the index by 4096, we put a given FreeEntry
        // directly into the memory segment it describes.
        phys_to_virt(index << 12)
    }

    fn get_index(&mut self, entry: *mut FreeEntry) -> u64 {
        // The index of a given FreeEntry is its "page index",
        // so just divide its address by 4096.
        virt_to_phys(entry) >> 12
    }
}

/// The Singleton [`PhysMemoryManager`] instance.
/// 
/// Starts unitialized, use [`api::init_phys_manager()`] to initialize.
static mut INSTANCE: MaybeUninit<PhysMemoryManager> = MaybeUninit::uninit();

pub fn init_phys_manager(kernel_header: &KernelHeader) {
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
    /// Create a new [`PhysMemoryManager`] from a given `memory_map`.
    pub fn new(memory_map: &mut [MemorySegment]) -> Self {
        verbose!("PhysManager", "Starting initialization");

        // find out the maximum address that is accessible according to the memory_map.
        let max_address = memory_map.iter()
            .map(|entry| entry.start + entry.page_count * 4096)
            .max().expect("Memory Map is empty");
        verbose!("PhysManager", "max_address={:#016X}", max_address);

        let storage = Storage::new(max_address >> 12, memory_map).into();

        let res = Self {
            lock: SpinLock::new(),
            free_lists: [null_mut(); MAX_ORDER+1].into(),
            storage,
        };

        // Inform the memory manager of every MemorySegment that is marked as free.
        for entry in memory_map.iter().filter(|&e| e.state == MemorySegmentState::Free) {
            verbose!("PhysManager", "Free segment {:#016X} - {:#016X}    {}", entry.start, entry.start + entry.page_count * 4096, entry.page_count);
            res.add_region(entry.start >> 12, entry.page_count);
        }

        #[cfg(feature="verbose-logging")]
        {
            for order in 0..MAX_ORDER+1 {
                let mut tmp = unsafe{&*res.free_lists.get()}[order];
                let mut count = 0;
                while !tmp.is_null() {
                    count += 1;
                    unsafe {
                        tmp = (*tmp).next;
                    }
                }

                verbose!("PhysManager", "{} regions of order {}", count, order);
            }
        }

        verbose!("PhysManager", "Initialized");

        res
    }

    /// Marks a given region as unallocated.
    /// 
    /// `index` and `page_count` don't need to fulfill any alignment requirements, 
    /// buddy splits will be done when necessary.
    fn add_region(&self, mut index: u64, mut page_count: u64) {
        let _guard = self.lock.lock();
        let storage = unsafe{&mut *self.storage.get()};
        let free_lists = unsafe{&mut *self.free_lists.get()};

        while page_count > 0 {
            // The maximum order that is allowed alignment-wise at the current index.
            let index_order = index.trailing_zeros();
            // The maximum order that can be filled with the number of remaining pages.
            let count_order = 63 - page_count.leading_zeros();
            // The order we will use.
            let order = index_order.min(count_order).min(MAX_ORDER as u32);

            Self::free_block(storage, free_lists, index, order);

            index += 1 << order;
            page_count -= 1 << order;
        }
    }

    /// Returns the index of the neighboring buddy that could be
    /// merged with.
    fn get_buddy_index(index: u64, order: u32) -> u64 {
        index ^ (1 << order)
    }

    /// Returns the index of the higher order buddy this buddy
    /// is contained in.
    fn get_combined_index(index: u64, order: u32) -> u64 {
        index & !(1 << order)
    }

    /// Returns the order that is needed to allocate `count` pages.
    fn get_size_order(count: u64) -> u32 {
        let order = 63 - count.leading_zeros();
        if count & (1 << order) != count {
            order + 1
        } else {
            order
        }
    }

    /// Removes a [`FreeEntry`] from the buddy list with the given `head`.
    /// 
    /// Note that this function will not clear the corresponding buddy bitmap entry.
    fn remove_buddy_list_entry(head: &mut *mut FreeEntry, entry: *mut FreeEntry) {
        unsafe {
            if (*entry).prev.is_null() {
                *head = (*entry).next;
            } else {
                (*(*entry).prev).next = (*entry).next;
            }
            if !(*entry).next.is_null() {
                (*(*entry).next).prev = (*entry).prev;
            }
        }
    }

    /// Adds a [`FreeEntry`] to the front of the buddy list with the given `head`.
    /// 
    /// Note that this function will not set the corresponding buddy bitmap entry.
    fn push_buddy_list_entry(head: &mut *mut FreeEntry, entry: *mut FreeEntry) {
        unsafe {
            if !(*head).is_null() {
                (*entry).next = *head;
                (**head).prev = entry;
            }
            *head = entry;
        }
    }

    /// Pops and returns the first entry of the buddy list with the given `head`.
    /// If the list is empty it returns `nullptr`.
    fn pop_buddy_list_entry(head: &mut *mut FreeEntry) -> *mut FreeEntry {
        unsafe {
            if (*head).is_null() {
                null_mut()
            } else {
                let tmp = *head;
                *head = (**head).next;
                if !(*head).is_null() {
                    (**head).prev = null_mut();
                }
                tmp
            }
        }
    }

    /// Mark a block at `index` with size order `order` as unallocated.
    /// 
    /// This function will automatically merge neighboring unallocated buddies when possible.
    fn free_block(storage: &mut Storage, free_lists: &mut [*mut FreeEntry], index: u64, order: u32) {
        // calculate bitmap position of the new block.
        let entry = index / 64;
        let bit = index % 64;
        let entry_ptr = storage.get_entry(index);

        // calculate bitmap position of the corresponding neighbor block.
        let buddy_index = Self::get_buddy_index(index, order);
        let buddy_entry = buddy_index / 64;
        let buddy_bit = buddy_index % 64;
        let buddy_ptr = storage.get_entry(buddy_index);

        let buddy_map = storage.get_buddy_map();

        // Merge if:
        // - The block to be freed is smaller than MAX_ORDER
        // - The bitmap entry of the neighbor is set (indicating that a free block of *some* order is present in the neighbor)
        // - The order of the neighboring FreeEntry is the same as ours.
        if order < MAX_ORDER as u32 && buddy_map[buddy_entry as usize] & (1 << buddy_bit) != 0 && unsafe{ (*buddy_ptr).order == order as usize } {
            buddy_map[buddy_entry as usize] &= !(1 << buddy_bit);
            // Remove the neighboring FreeEntry.
            Self::remove_buddy_list_entry(&mut free_lists[order as usize], buddy_ptr);
            // Recursively free the next higher order block.
            Self::free_block(storage, free_lists, Self::get_combined_index(index, order), order+1);
        } else {
            // Merging not possible, just add the new FreeEntry to the list.
            buddy_map[entry as usize] |= 1 << bit;
            unsafe{entry_ptr.write(FreeEntry {
                order: order as usize,
                next: null_mut(),
                prev: null_mut(),
            })};
            Self::push_buddy_list_entry(&mut free_lists[order as usize], entry_ptr);
        }
    }

    /// Allocate a block with size order `order` and return its index.
    /// 
    /// This function will automatically split higher order blocks when needed.
    fn alloc_block(storage: &mut Storage, free_lists: &mut [*mut FreeEntry], order: u32) -> u64 {
        let entry = Self::pop_buddy_list_entry(&mut free_lists[order as usize]);

        // No block of the requested order is available, try to split a higher order block.
        if entry.is_null() {
            // If the requested order is MAX_ORDER, we cannot split a higher order block.
            if (order as usize) == MAX_ORDER {
                panic!("Out of physical memory");
            }

            // recursively allocate a block of the next higher order.
            let higher_block = Self::alloc_block(storage, free_lists, order+1);
            // calculate the index of the higher half block.
            let buddy_index = Self::get_buddy_index(higher_block, order);
            let buddy_entry = buddy_index / 64;
            let buddy_bit = buddy_index % 64;
            let buddy_ptr = storage.get_entry(buddy_index);

            let buddy_map = storage.get_buddy_map();

            // mark the higher half block as free
            buddy_map[buddy_entry as usize] |= 1 << buddy_bit;

            unsafe{buddy_ptr.write(FreeEntry {
                order: order as usize,
                next: null_mut(),
                prev: null_mut(),
            })};
            Self::push_buddy_list_entry(&mut free_lists[order as usize], buddy_ptr);

            // return the lower half block
            higher_block
        } else {
            // block of the requested order is available, remove it from the list and return it.
            let index = storage.get_index(entry);
            let entry = index / 64;
            let bit = index % 64;

            let buddy_map = storage.get_buddy_map();

            buddy_map[entry as usize] &= !(1 << bit);
            index
        }
    }

    /// Frees a single page of physical memory at the given `addr`.
    pub fn free_page(&self, addr: u64) {
        let _guard = self.lock.lock();
        let storage = unsafe{&mut *self.storage.get()};
        let free_lists = unsafe{&mut *self.free_lists.get()};

        Self::free_block(storage, free_lists, addr >> 12, 0);
    }

    /// Frees a contiguous region of `count` pages of physical memory at the given `addr`.
    /// 
    /// Must only be called with regions allocated with [`Self::alloc_linear_pages()`].
    pub fn free_linear_pages(&self, addr: u64, count: u64) {
        let _guard = self.lock.lock();
        let storage = unsafe{&mut *self.storage.get()};
        let free_lists = unsafe{&mut *self.free_lists.get()};

        Self::free_block(storage, free_lists, addr >> 12, Self::get_size_order(count));
    }

    /// Frees several single-page blocks, each address given in one entry of `addresses`.
    pub fn free_pages(&self, addresses: &[u64]) {
        let _guard = self.lock.lock();
        let storage = unsafe{&mut *self.storage.get()};
        let free_lists = unsafe{&mut *self.free_lists.get()};

        for addr in addresses {
            Self::free_block(storage, free_lists, addr >> 12, 0);
        }
    }

    /// Allocates and returns the physical address of a single memory page.
    pub fn alloc_page(&self) -> u64 {
        let _guard = self.lock.lock();
        let storage = unsafe{&mut *self.storage.get()};
        let free_lists = unsafe{&mut *self.free_lists.get()};

        Self::alloc_block(storage, free_lists, 0) << 12
    }

    /// Allocates and returns the physical address of a contiguous region of memory with `count` pages.
    pub fn alloc_linear_pages(&self, count: u64) -> u64 {
        let _guard = self.lock.lock();
        let storage = unsafe{&mut *self.storage.get()};
        let free_lists = unsafe{&mut *self.free_lists.get()};

        Self::alloc_block(storage, free_lists, Self::get_size_order(count)) << 12
    }

    /// Allocates `addresses.len()` single-page blocks and returns each address in the given slice. 
    /// 
    /// The blocks will not be contiguous in physical memory.
    pub fn alloc_pages(&self, addresses: &mut [u64]) {
        let _guard = self.lock.lock();
        let storage = unsafe{&mut *self.storage.get()};
        let free_lists = unsafe{&mut *self.free_lists.get()};

        for out_addr in addresses {
            *out_addr = Self::alloc_block(storage, free_lists, 0) << 12;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// [`PhysManagerStorage`] implementation that allows testing the [`PhysMemoryManager`] in unit tests.
    /// 
    /// For the normal kernel implementation, see [`InlineStorage`].
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
            // instead of calculating the physical address of a FreeEntry,
            // calculate the offset into the memory buffer.
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
