use crate::memory;
use core::mem::size_of;
use core::ptr::null_mut;

/*
    On x86_64, the GDT is basically completely useless, the only used feature is checking the privilege level of
    the code segment. Some weird rules however don't get disabled, even though they are completely, utterly useless.
    E.g. it is still forbidden to MOV a selector into the stack segment that has a different privilege level, even though
    that privilege level is never actually checked afterwards. When writing to ss through an IRET though, the SS value is
    written completely unchecked.

    Since a program will never need to change CS, DS, ES and SS while running, we only ever need to change those values
    through IRET (used when switching processes), which does not check any rules for SS, meaning we don't need any user data descriptors.

    The following selectors will be used:
    - Kernel Mode:
        CS = SELECTOR_KERNEL_CODE
        DS = 0
        ES = 0
        SS = 0
    - User Mode:
        CS = SELECTOR_USER_CODE
        DS = 0
        ES = 0
        SS = 0
*/

pub const SELECTOR_NULL: u16 = 0;
pub const SELECTOR_KERNEL_CODE: u16 = 8;
pub const SELECTOR_USER_CODE: u16 = 16 | 3;

/// Pointer to the Task State Segment, which is mainly used to determine which stack should
/// be used for interrupts.
static mut TSS: *mut Tss = null_mut();

pub fn init() {
    info!("GDT", "Initializing...");

    let mem = memory::phys_to_virt::<GDTEntry>(memory::phys_manager().alloc_page());
    verbose!("GDT", "GDT at {:#016X}", mem as u64);

    let tss_mem = memory::phys_to_virt::<Tss>(memory::phys_manager().alloc_page());

    unsafe {
        mem.offset(0).write(GDTEntry::null());
        mem.offset(1).write(GDTEntry::new_code(false));
        mem.offset(2).write(GDTEntry::new_code(true));

        // The TSS needs an entry in the GDT that points to the actual TSS memory.
        // This entry takes up two GDT entry slots.
        let tss_entry = GDTEntryTSS {
            limit0: size_of::<Tss>() as u16 - 1,
            base0: tss_mem as u16,
            base1: ((tss_mem as u64) >> 16) as u8,
            type_dpl_p: 0b10001001,
            limi1: 0,
            base2: ((tss_mem as u64) >> 24) as u8,
            base3: ((tss_mem as u64) >> 32) as u32,
            reserved: 0,
        };
        (mem.offset(3) as *mut GDTEntryTSS).write(tss_entry);

        let tss = Tss {
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

    // This structure is used by LGDT.
    // base + limit is the last *accessible* byte in the GDT, so
    // it has to be one less than the *size*.
    let desc = Gdtr {
        base: mem as u64,
        limit: 5 * 8 - 1,
    };
    unsafe{asm!(
        "lgdt [{desc}]",            // use the newly created GDT
        "mov ds, {null:x}",         // load every data segment register with null descriptors
        "mov es, {null:x}",
        "mov ss, {null:x}",
        "push {kcode}",             // push the kernel code selector
        "lea {tmp}, [1f + rip]",    // find out the absolute address of the 1: label below
        "push {tmp}",
        "retfq",                    // RETF pops off the new RIP and CS from the stack and uses them.
                                    // This is needed because directly writing to the CS segment register is
                                    // impossible.
        "1: nop",

        desc=in(reg) &desc as *const _,
        kcode=const SELECTOR_KERNEL_CODE,
        null=in(reg) SELECTOR_NULL,
        tmp=lateout(reg) _,
    )};

    unsafe{asm!(
        "ltr {sel:x}",              // Load the selector for the GDT entry that describes the location of the TSS.
                                    // Why this indirection is needed is beyond me.
        sel=in(reg) 3*8,
    )};

    info!("GDT", "Initialized");
}

/// Sets the address of the stack used for most interrupts.
pub fn set_ist1(val: u64) {
    unsafe {
        (*TSS).ist1 = val;
    }
}

#[repr(C, packed)]
struct Gdtr {
    pub limit: u16,
    pub base: u64,
}

#[repr(transparent)]
struct GDTEntry {
    _data: u64,
}

impl GDTEntry {
    fn new_code(user_mode: bool) -> Self {
        let mut _data = if user_mode {
            DESC_CODE_BASE | DESC_USER_DPL
        } else {
            DESC_CODE_BASE
        };

        Self{_data}
    }
    fn null() -> Self {
        Self{_data: 0}
    }
}

/// L and P set
const DESC_CODE_BASE: u64 = (1 << 43) | (1 << 44) | (1 << 47) | (1 << 53);

const DESC_USER_DPL: u64 = 3 << 45;


#[repr(C, packed)]
struct GDTEntryTSS {
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
struct Tss {
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
