use crate::memory;
use core::mem::size_of;
use core::ptr::null_mut;

pub const SELECTOR_KERNEL_CODE: u16 = 8;
pub const SELECTOR_KERNEL_DATA: u16 = 16;
pub const SELECTOR_USER_CODE: u16 = 24 | 3;
pub const SELECTOR_USER_DATA: u16 = 32 | 3;

static mut TSS: *mut TSS = null_mut();

pub fn init() {
    info!("GDT", "Initializing...");

    let mem = memory::phys_to_virt::<GDTEntry>(memory::phys_manager().alloc_page());
    verbose!("GDT", "GDT at {:#016X}", mem as u64);

    let tss_mem = memory::phys_to_virt::<TSS>(memory::phys_manager().alloc_page());

    unsafe {
        mem.offset(0).write(GDTEntry::null());
        mem.offset(1).write(GDTEntry::new(true, false));
        mem.offset(2).write(GDTEntry::new(false, false));
        mem.offset(3).write(GDTEntry::new(true, true));
        mem.offset(4).write(GDTEntry::new(false, true));

        let tss_entry = GDTEntryTSS {
            limit0: size_of::<TSS>() as u16 - 1,
            base0: tss_mem as u16,
            base1: ((tss_mem as u64) >> 16) as u8,
            type_dpl_p: 0b10001001,
            limi1: 0,
            base2: ((tss_mem as u64) >> 24) as u8,
            base3: ((tss_mem as u64) >> 32) as u32,
            reserved: 0,
        };
        (mem.offset(5) as *mut GDTEntryTSS).write(tss_entry);

        let tss = TSS {
            reserved0: 0,
            rsp0: 0,
            rsp1: 0,
            rsp2: 0,
            reserved1: 0,
            ist1: 0,
            ist2: 0,
            ist3: 0,
            ist4: 0,
            ist5: 0,
            ist6: 0,
            ist7: 0,
            reserved2: 0,
            reserved3: 0,
        };
        tss_mem.write(tss);

        TSS = tss_mem;
    }

    let desc = GDTR {
        base: mem as u64,
        limit: 7 * 8 - 1,
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

    unsafe{asm!(
        "ltr {sel:x}",
        sel=in(reg) 5*8,
    )};

    info!("GDT", "Initialized");
}

pub fn set_ist1(val: u64) {
    unsafe {
        (*TSS).ist1 = val;
    }
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


#[repr(C, packed)]
pub struct GDTEntryTSS {
    limit0: u16,
    base0: u16,
    base1: u8,
    type_dpl_p: u8,
    limi1: u8,
    base2: u8,
    base3: u32,
    reserved: u32,
}

#[repr(C, packed)]
pub struct TSS {
    reserved0: u32,
    rsp0: u64,
    rsp1: u64,
    rsp2: u64,
    reserved1: u64,
    ist1: u64,
    ist2: u64,
    ist3: u64,
    ist4: u64,
    ist5: u64,
    ist6: u64,
    ist7: u64,
    reserved2: u64,
    reserved3: u32,
}
