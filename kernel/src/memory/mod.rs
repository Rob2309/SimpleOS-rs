
mod phys_manager;
pub use phys_manager::init_phys_manager;
pub use phys_manager::phys_manager;

mod virt_manager;
pub use virt_manager::init_virt_manager;
pub use virt_manager::set_high_mem_base;
pub use virt_manager::phys_to_virt;
pub use virt_manager::virt_to_phys;
