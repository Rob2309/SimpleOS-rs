use core::{cell::UnsafeCell, marker::PhantomData, ops::{Deref, DerefMut}, sync::atomic::{AtomicBool, Ordering}};

pub trait Lock {
    fn try_lock(&self) -> Option<LockGuard<Self>>;
    fn lock(&self) -> LockGuard<Self> {
        loop {
            let lg = self.try_lock();
            if let Some(lg) = lg {
                return lg;
            }
        }
    }

    fn unlock(&self);
}

pub struct LockGuard<'a, L: Lock + ?Sized> {
    lock: &'a L,
}

impl<'a, L: Lock + ?Sized> Drop for LockGuard<'a, L> {
    fn drop(&mut self) {
        self.lock.unlock();
    }
}

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
        if self.locked.swap(true, Ordering::Acquire) == false {
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
