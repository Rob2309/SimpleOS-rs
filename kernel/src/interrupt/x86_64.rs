use core::ptr::null_mut;

use crate::{arch::gdt, memory};

static mut IDT: *mut IDTEntry = null_mut();

pub fn platform_init() {
    let int_stack = memory::phys_to_virt::<u8>(memory::phys_manager().alloc_linear_pages(4)) as u64;
    gdt::set_ist1(int_stack + 4 * 4096);

    let idt = memory::phys_to_virt::<IDTEntry>(memory::phys_manager().alloc_page());
    unsafe {
        idt.write_bytes(0, 4096);
        IDT = idt;
    }

    unsafe {
        let idt_desc = IDTDesc {
            limit: 4095,
            address: idt as u64,
        };
        asm!(
            "lidt [{idt_desc}]",
            idt_desc=in(reg) &idt_desc as *const _,
        );
    }

    macro_rules! isr {
        ($name:ident, $number:literal) => {
            set_idt_entry($number, $name);
        };
        ($name:ident, $number:literal, error) => {
            set_idt_entry($number, $name);
        }
    }
    include!("set_isrs.rs");
}

fn set_idt_entry(index: u8, handler: extern "C" fn()) {
    unsafe {
        IDT.offset(index as isize).write(IDTEntry {
            offset_low: handler as u16,
            target_selector: gdt::SELECTOR_KERNEL_CODE,
            ist: 1,
            type_dpl_p: 0b10001110,
            offset_mid: ((handler as u64) >> 16) as u16,
            offset_high: ((handler as u64) >> 32) as u32,
            reserved: 0,
        });
    }
}

extern "sysv64" fn isr_common_handler(info: &mut InterruptInfo) {
    warning!("IDT", "Interrupt {:#02X} occured", info.int_number);
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

#[repr(C, packed)]
struct InterruptInfo {
    r15: u64,
    r14: u64,
    r13: u64,
    r12: u64,
    r11: u64,
    r10: u64,
    r9: u64,
    r8: u64,
    rbp: u64,
    rdi: u64,
    rsi: u64,
    rdx: u64,
    rcx: u64,
    rbx: u64,
    rax: u64,
    int_number: u64,
    error_code: u64,
    rip: u64,
    cs: u64,
    rflags: u64,
    rsp: u64,
    ss: u64,
}

#[naked]
extern "C" fn isr_common_stub() {
    unsafe{asm!(
        "push rax",
        "push rbx",
        "push rcx",
        "push rdx",
        "push rsi",
        "push rdi",
        "push rbp",
        "push r8",
        "push r9",
        "push r10",
        "push r11",
        "push r12",
        "push r13",
        "push r14",
        "push r15",

        "mov rdi, rsp",
        "sub rsp, 8",
        
        "call {common}",

        "add rsp, 8",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop r11",
        "pop r10",
        "pop r9",
        "pop r8",
        "pop rbp",
        "pop rdi",
        "pop rsi",
        "pop rdx",
        "pop rcx",
        "pop rbx",
        "pop rax",
        "add rsp, 16",

        "iretq",

        common = sym isr_common_handler,

        options(noreturn)
    )};
}

macro_rules! isr {
    ($name:ident, $number:literal) => {
        #[naked]
        extern "C" fn $name() {
            unsafe{asm!(
                "push 0",
                "push {num}",
                "jmp {common}",

                num=const $number,
                common=sym isr_common_stub,
                
                options(noreturn)
            )};
        }
    };
    ($name:ident, $number:literal, error) => {
        #[naked]
        extern "C" fn $name() {
            unsafe{asm!(
                "push {num}",
                "jmp {common}",

                num=const $number,
                common=sym isr_common_stub,
                
                options(noreturn)
            )};
        }
    }
}
include!("isrs.rs");
