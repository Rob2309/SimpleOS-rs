use crate::{arch::gdt, memory};


pub fn platform_init() {
    let int_stack = memory::phys_to_virt::<u8>(memory::phys_manager().alloc_linear_pages(4)) as u64;
    gdt::set_ist1(int_stack + 4 * 4096);

    let idt = memory::phys_to_virt::<IDTEntry>(memory::phys_manager().alloc_page());

    let ff_handler = isr_ff_handler as u64;

    unsafe {
        let idt_desc = IDTDesc {
            limit: 4095,
            address: idt as u64,
        };
        asm!(
            "lidt [{idt_desc}]",
            idt_desc=in(reg) &idt_desc as *const _,
        );

        idt.offset(0xFF).write(IDTEntry {
            offset_low: ff_handler as u16,
            target_selector: gdt::SELECTOR_KERNEL_CODE,
            ist: 1,
            type_dpl_p: 0b10001110,
            offset_mid: (ff_handler >> 16) as u16,
            offset_high: (ff_handler >> 32) as u32,
            reserved: 0,
        });
    }

    let res: u64;
    unsafe{asm!(
        "int 0xFF",
        lateout("rax") res
    )};
    assert!(res == 0xABABABAB);
}

#[naked]
extern "C" fn isr_ff_handler() {
    unsafe{asm!(
        "cli",
        "mov rax, 0xABABABAB",
        "iretq",
        options(noreturn)
    )};
}

#[repr(C, packed)]
struct IDTEntry {
    offset_low: u16,
    target_selector: u16,
    ist: u8,
    type_dpl_p: u8,
    offset_mid: u16,
    offset_high: u32,
    reserved: u32,
}

#[repr(C, packed)]
struct IDTDesc {
    limit: u16,
    address: u64,
}
