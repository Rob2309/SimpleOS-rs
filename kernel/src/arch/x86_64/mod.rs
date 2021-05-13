
pub mod gdt;

pub fn init_platform() {
    gdt::init();
}
