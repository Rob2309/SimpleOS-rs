use core::{cell::UnsafeCell, marker::PhantomData, ops::{Deref, DerefMut}, sync::atomic::{AtomicBool, Ordering}};

pub trait Lock<T> {
    fn try_lock(&self) -> Option<LockGuard<T, Self>>;
    fn lock(&self) -> LockGuard<T, Self> {
        loop {
            let lg = self.try_lock();
            if let Some(lg) = lg {
                return lg;
            }
        }
    }

    fn unlock(&self);

    unsafe fn inner(&self) -> &mut T;
}

pub struct LockGuard<'a, T, L: Lock<T> + ?Sized> {
    lock: &'a L,
    _p: PhantomData<T>,
}

impl<'a, T, L: Lock<T> + ?Sized> Drop for LockGuard<'a, T, L> {
    fn drop(&mut self) {
        self.lock.unlock();
    }
}

impl<'a, T, L: Lock<T> + ?Sized> Deref for LockGuard<'a, T, L> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe{ self.lock.inner() }
    }
}

impl<'a, T, L: Lock<T> + ?Sized> DerefMut for LockGuard<'a, T, L> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe{ self.lock.inner() }
    }
}

pub struct SpinLock<T> {
    locked: AtomicBool,
    content: UnsafeCell<T>,
}

impl<T> SpinLock<T> {
    pub fn new(init: T) -> Self {
        Self {
            locked: AtomicBool::new(false),
            content: init.into(),
        }
    }
}

impl<T> Lock<T> for SpinLock<T> {
    fn try_lock(&self) -> Option<LockGuard<T, Self>> {
        if self.locked.swap(true, Ordering::Acquire) == false {
            Some(LockGuard {
                lock: self,
                _p: PhantomData,
            })
        } else {
            None
        }
    }

    fn unlock(&self) {
        self.locked.store(false, Ordering::Release);
    }

    unsafe fn inner(&self) -> &mut T {
        &mut *self.content.get()
    }
}
