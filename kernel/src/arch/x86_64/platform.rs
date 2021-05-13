use super::gdt;

pub fn init() {
    gdt::init();
}