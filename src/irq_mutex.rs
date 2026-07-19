//! Spin mutex that prevents same-CPU preemption while locked.
//!
//! A plain spinlock can deadlock on a preemptive single-core kernel: task A
//! owns the lock, the timer switches to task B, and task B spins on the same
//! lock while A cannot run. `IrqMutex` saves IF, disables interrupts before
//! acquisition, then unlocks before restoring the saved IF state.

use core::ops::{Deref, DerefMut};
use spin::{Mutex, MutexGuard};
use x86_64::instructions::interrupts;

pub struct IrqMutex<T: ?Sized> {
    inner: Mutex<T>,
}

impl<T> IrqMutex<T> {
    pub const fn new(value: T) -> Self {
        Self {
            inner: Mutex::new(value),
        }
    }
}

impl<T: ?Sized> IrqMutex<T> {
    pub fn lock(&self) -> IrqMutexGuard<'_, T> {
        let restore_interrupts = interrupts::are_enabled();
        interrupts::disable();
        IrqMutexGuard {
            guard: Some(self.inner.lock()),
            restore_interrupts,
        }
    }

    /// Release a poisoned console lock during a fatal panic.
    ///
    /// # Safety
    ///
    /// The caller must ensure no live guard will subsequently access the
    /// protected value. The panic handlers satisfy this by halting the kernel.
    pub unsafe fn force_unlock(&self) {
        unsafe { self.inner.force_unlock() };
    }
}

pub struct IrqMutexGuard<'a, T: ?Sized> {
    // Option lets Drop release the spinlock before it restores IF.
    guard: Option<MutexGuard<'a, T>>,
    restore_interrupts: bool,
}

impl<T: ?Sized> Deref for IrqMutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.guard.as_deref().expect("IRQ mutex guard missing")
    }
}

impl<T: ?Sized> DerefMut for IrqMutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.guard.as_deref_mut().expect("IRQ mutex guard missing")
    }
}

impl<T: ?Sized> Drop for IrqMutexGuard<'_, T> {
    fn drop(&mut self) {
        drop(self.guard.take());
        if self.restore_interrupts {
            interrupts::enable();
        }
    }
}
