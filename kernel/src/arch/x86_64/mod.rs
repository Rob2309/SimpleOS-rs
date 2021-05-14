
pub mod gdt;
pub mod interrupt;
pub mod virt_manager;

pub fn init_platform() {
    gdt::init(1);
    gdt::init_core(0);

    interrupt::init();
    interrupt::init_core(0);
}

pub fn init_secondary_core(core_id: usize) {
    gdt::init_core(core_id);
    interrupt::init_core(core_id);
}
