use crate::arch::interrupt as arch;

pub fn init() {
    info!("IDT", "Initializing...");

    arch::init();

    info!("IDT", "Initialized...");
}
