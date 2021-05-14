use core::ptr::null_mut;

use crate::{arch::gdt, memory};

/// Pointer to the low-level Interrupt Descriptor Table.
static mut IDT: *mut IDTEntry = null_mut();
/// Array of high-level handlers that are called for the respective interrupts.
static mut HANDLERS: [fn (&mut InterruptInfo); 256] = [isr_default_handler; 256];

pub fn init() {
    // Allocate a 16KB interrupt stack that will be used by every interrupt.
    // This ensures that every interrupt has 16 KB stack space in every situation,
    // but also makes nested interrupts impossible, since the two interrupts would corrupt each others
    // stack space.
    let int_stack = memory::phys_to_virt::<u8>(memory::phys_manager().alloc_linear_pages(4)) as u64;
    gdt::set_ist1(int_stack + 4 * 4096);

    // Allocate 256 * 16 bytes for the IDT, exactly one page.
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
            "lidt [{idt_desc}]",                    // use the newly created IDT.
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
    // This file includes 256 isr!(...) macros, one for every possible interrupt.
    // So for every possible interrupt number, the respective stub will be registered to the IDT.
    include!("set_isrs.rs");
}

/// Sets the low-level stub for a given interrupt index. 
/// This function should only ever be used on IDT initialization, 
/// as the required low-level code is always the same.
fn set_idt_entry(index: u8, handler: extern "C" fn()) {
    unsafe {
        IDT.offset(index as isize).write(IDTEntry {
            offset_low: handler as usize as u16,
            target_selector: gdt::SELECTOR_KERNEL_CODE,
            ist: 1,
            type_dpl_p: 0b10001110,
            offset_mid: ((handler as usize) >> 16) as u16,
            offset_high: ((handler as usize) >> 32) as u32,
            reserved: 0,
        });
    }
}

/// Sets the high-level interrupt handler for a given interrupt index.
pub fn set_isr_handler(index: u8, handler: fn(&mut InterruptInfo)) {
    unsafe {
        HANDLERS[index as usize] = handler;
    }
}

/// The default high-level interrupt handler. Just prints out a warning and returns.
fn isr_default_handler(info: &mut InterruptInfo) {
    warning!("IDT", "Interrupt {:#02X} occured and no handler installed", info.int_number);
}

/// The common interrupt handler entry point that will be called by the 
/// low-level stubs.
extern "sysv64" fn isr_common_handler(info: &mut InterruptInfo) {
    unsafe {
        HANDLERS[info.int_number as usize](info);
    }
}

#[repr(C, packed)]
struct IDTEntry {
    /// Bits 0-15 of the interrupt handler function.
    offset_low: u16,
    /// Code selector (used to set the privilege level at which the interrupt handler runs).
    target_selector: u16,
    /// If not 0, the corresponding interrupt stack in the TSS (see [`gdt`]) 
    /// will be used.
    ist: u8,
    /// Type and P will always be the same, dpl is the privilege level at which 
    /// firing this interrupt via INT instruction is allowed.
    type_dpl_p: u8,
    /// Bits 16-31 of the interrupt handler function.
    offset_mid: u16,
    /// Bits 32-63 of the interrupt handler function.
    offset_high: u32,
    /// Must be zero
    reserved: u32,
}

#[repr(C, packed)]
struct IDTDesc {
    limit: u16,
    address: u64,
}

/// This structure is passed to the high-level interrupt handlers. It can be modified by those handlers
/// to change the processor state that will be set when returning.
#[repr(C)]
pub struct InterruptInfo {
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
    /// The interrupt number that was fired.
    /// Can be used to distinguish interrupts when multiple 
    /// numbers have the same high-level handler.
    int_number: u64,
    /// Some processor exceptions push an error code.
    error_code: u64,
    rip: u64,
    cs: u64,
    rflags: u64,
    rsp: u64,
    ss: u64,
}

/// The common stub code for every low-level interrupt handler.
#[naked]
extern "C" fn isr_common_stub() {
    // At this point, everything up to int_number in InterruptInfo
    // will be on the stack.
    unsafe{asm!(
        // push all standard registers onto the stack.
        // Note that the push order is opposite to the InterruptInfo struct,
        // as the stack grows downwards.
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

        // in the SystemV ABI, rdi holds the first function argument
        "mov rdi, rsp",
        // the SystemV ABI expects a 16-Byte aligned stack on function entry.
        // This and the CALL instruction will together push 16 Bytes to the stack,
        // resulting in the correct alignment.
        "sub rsp, 8",
        
        // Call the common high-level handler
        "call {common}",

        // restore the pre-interrupt processor state.
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
        // pop int_number and error_code off the stack.
        "add rsp, 16",

        // resume the normal program execution
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
                "push 0",       // push fake error code
                "push {num}",   // push interrupt number
                "jmp {common}", // jump to common stub

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
                "push {num}",   // push interrupt number
                "jmp {common}", // jump to common stub

                num=const $number,
                common=sym isr_common_stub,
                
                options(noreturn)
            )};
        }
    }
}
// This file includes 256 isr!(...) macros, one for every possible interrupt.
// So for every possible interrupt number, the respective stub will be generated.
// This file cannot be the same as the one used in init() because rusts macro system
// is very limited.
include!("isrs.rs");
