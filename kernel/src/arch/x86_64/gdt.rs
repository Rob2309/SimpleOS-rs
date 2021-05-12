use crate::memory;

pub const SELECTOR_KERNEL_CODE: u16 = 8;
pub const SELECTOR_KERNEL_DATA: u16 = 16;
pub const SELECTOR_USER_CODE: u16 = 24 | 3;
pub const SELECTOR_USER_DATA: u16 = 32 | 3;

pub fn init() {
    let mem = memory::phys_to_virt::<GDTEntry>(memory::phys_manager().alloc_page());

    unsafe {
        mem.offset(0).write(GDTEntry::null());
        mem.offset(1).write(GDTEntry::new(true, false));
        mem.offset(2).write(GDTEntry::new(false, false));
        mem.offset(3).write(GDTEntry::new(true, true));
        mem.offset(4).write(GDTEntry::new(false, true));
    }

    let desc = GDTR {
        base: mem as u64,
        limit: 5 * 8 - 1,
    };
    unsafe{asm!(
        "lgdt [{desc}]",
        "mov ds, {kdata:x}",
        "mov es, {kdata:x}",
        "mov ss, {kdata:x}",
        "push {kcode}",
        "lea {tmp}, [1f + rip]",
        "push {tmp}",
        "retf",
        "1: nop",

        desc=in(reg) &desc as *const _,
        kdata=in(reg) SELECTOR_KERNEL_DATA,
        kcode=const SELECTOR_KERNEL_CODE,
        tmp=lateout(reg) _,
    )};
} 

#[repr(C)]
pub struct GDTR {
    pub limit: u16,
    pub base: u64,
}

#[repr(transparent)]
pub struct GDTEntry {
    data: u64,
}

impl GDTEntry {
    pub fn new(code: bool, user_mode: bool) -> Self {
        let mut data = DESC_P | DESC_L;

        if code {
            data |= DESC_CODE_BASE;
        } else {
            data |= DESC_DATA_BASE;
        }
        if user_mode {
            data |= DESC_USER_DPL;
        }

        Self{data}
    }
    pub fn null() -> Self {
        Self{data: 0}
    }
}

const DESC_CODE_BASE: u64 = (1 << 43) | (1 << 44);
const DESC_DATA_BASE: u64 = 1 << 44;

const DESC_L: u64 = 1 << 53;
const DESC_P: u64 = 1 << 47;

const DESC_USER_DPL: u64 = 3 << 45;
