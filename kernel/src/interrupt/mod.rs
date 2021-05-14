use crate::arch::interrupt as arch;

/// Initializes whatever interrupt mechanism the platform uses.
/// 
/// Has to be called after [`crate::memory::init_virt_manager()`] and [`crate::memory::init_phys_manager()`]
/// as it might need to allocate memory for interrupt tables.
pub fn init() {
    info!("IDT", "Initializing...");

    arch::init();

    info!("IDT", "Initialized...");
}
