use crate::memory;

pub const SELECTOR_KERNEL_CODE: u16 = 8;
pub const SELECTOR_KERNEL_DATA: u16 = 16;
pub const SELECTOR_USER_CODE: u16 = 24 | 3;
pub const SELECTOR_USER_DATA: u16 = 32 | 3;

pub fn init() {
    info!("GDT", "Initializing...");

    let mem = memory::phys_to_virt::<GDTEntry>(memory::phys_manager().alloc_page());
    verbose!("GDT", "GDT at {:#016X}", mem as u64);

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
        "retfq",
        "1: nop",

        desc=in(reg) &desc as *const _,
        kdata=in(reg) SELECTOR_KERNEL_DATA,
        kcode=const SELECTOR_KERNEL_CODE,
        tmp=lateout(reg) _,
    )};

    info!("GDT", "Initialized");
} 

#[repr(C, packed)]
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
        let mut data = if code {
            DESC_CODE_BASE
        } else {
            DESC_DATA_BASE
        };

        if user_mode {
            data |= DESC_USER_DPL;
        }

        Self{data}
    }
    pub fn null() -> Self {
        Self{data: 0}
    }
}

/// L and P set
const DESC_CODE_BASE: u64 = (1 << 43) | (1 << 44) | (1 << 47) | (1 << 53);
/// for some reason, W has to be set, even though the spec states that attributes are ignored
const DESC_DATA_BASE: u64 = (1 << 47) | (1 << 44) | (1 << 41);

const DESC_USER_DPL: u64 = 3 << 45;
