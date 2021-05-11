use core::sync::atomic::{AtomicBool, Ordering};

/// Interface for generic Locks.
pub trait Lock {
    /// Try to lock, return a [`LockGuard`] if successful.
    fn try_lock(&self) -> Option<LockGuard<Self>>;
    /// Block until the lock can be acquired.
    fn lock(&self) -> LockGuard<Self> {
        loop {
            let lg = self.try_lock();
            if let Some(lg) = lg {
                return lg;
            }
        }
    }

    /// Unlock the lock.
    fn unlock(&self);
}

/// Automatically unlocks a lock when dropped.
pub struct LockGuard<'a, L: Lock + ?Sized> {
    lock: &'a L,
}

impl<'a, L: Lock + ?Sized> Drop for LockGuard<'a, L> {
    fn drop(&mut self) {
        self.lock.unlock();
    }
}

/// Basic kernel SpinLock.
pub struct SpinLock {
    locked: AtomicBool,
}

impl SpinLock {
    pub fn new() -> Self {
        Self {
            locked: AtomicBool::new(false),
        }
    }
}

impl Lock for SpinLock {
    fn try_lock(&self) -> Option<LockGuard<Self>> {
        if !self.locked.swap(true, Ordering::Acquire) {
            Some(LockGuard {
                lock: self,
            })
        } else {
            None
        }
    }

    fn unlock(&self) {
        self.locked.store(false, Ordering::Release);
    }
}
