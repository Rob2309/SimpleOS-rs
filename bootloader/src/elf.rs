use core::{mem::size_of, slice};

pub fn get_size(image: *const u8) -> usize {
    let header = unsafe { &*(image as *const Header) };

    let mut size = 0usize;

    let ph_list = unsafe { slice::from_raw_parts(image.offset(header.ph_offset as isize) as *const SegmentHeader, header.ph_entry_count as usize) };
    for s in ph_list {
        if s.seg_type == SEGTYPE_LOAD {
            let offs = (s.virt_addr + s.virt_size) as usize;
            if offs > size {
                size = offs;
            }
        }
    }

    size
}

#[cfg(debug_assertions)]
unsafe fn strcmp(mut a: *const u8, mut b: *const u8) -> bool {
    while *a == *b {
        if *a == b'\0' {
            return true;
        }

        a = a.wrapping_add(1);
        b = b.wrapping_add(1);
    }

    false
}

#[cfg(debug_assertions)]
pub fn get_text_addr(image: *const u8, process: *const u8) -> u64 {
    let header = unsafe { &*(image as *const Header) };

    let sh_list = unsafe { slice::from_raw_parts(image.offset(header.sh_offset as isize) as *const SectionHeader, header.sh_entry_count as usize) };
    let name_table = unsafe { image.offset(sh_list[header.name_string_table_index as usize].file_offset as isize) };
    for s in sh_list {
        if s.sec_type == SHT_PROGBITS {
            let name = unsafe { name_table.offset(s.name_offset as isize) };

            if unsafe {strcmp(name, ".text\0".as_ptr())} {
                return process as u64 + s.virt_addr;
            }
        }
    }
    
    0
}

pub fn prepare(image: *const u8, dest: *mut u8) -> u64 {
    let header = unsafe { &*(image as *const Header) };

    let ph_list = unsafe { slice::from_raw_parts(image.offset(header.ph_offset as isize) as *const SegmentHeader, header.ph_entry_count as usize) };
    for s in ph_list {
        if s.seg_type == SEGTYPE_LOAD {
            unsafe {
                let src = image.offset(s.data_offset as isize);
                let dst = dest.offset(s.virt_addr as isize);

                dst.copy_from_nonoverlapping(src, s.data_size as usize);
                dst.offset(s.data_size as isize).write_bytes(0, (s.virt_size - s.data_size) as usize);
            }
        } else if s.seg_type == SEGTYPE_DYNAMIC {
            let mut rela_addr = 0;
            let mut rela_count = 0;

            let mut dyn_entry = unsafe{dest.offset(s.virt_addr as isize) as *const DynamicEntry};
            loop {
                let de = unsafe{&*dyn_entry};
                match de.tag {
                    0 => break,
                    DE_TAG_RELA => {
                        rela_addr = de.value;
                    }
                    DE_TAG_RELASZ => {
                        rela_count = de.value / size_of::<RelA>() as u64;
                    }
                    _ => {}
                }

                dyn_entry = unsafe{dyn_entry.offset(1)};
            }

            let mut rela_entry = unsafe{dest.offset(rela_addr as isize) as *const RelA};
            for _ in 0..rela_count {
                let rela = unsafe{&*rela_entry};

                let rel_type = rela.info as u32;
                let target = dest as u64 + rela.addr;
                let addend = (rela.addend as u64).wrapping_add(dest as u64);

                match rel_type {
                    R_RELATIVE => {
                        unsafe {
                            *(target as *mut u64) = addend;
                        }
                    }
                    _ => panic!("Unsupported relocation ({}) while preparing kernel image", rel_type)
                }

                rela_entry = rela_entry.wrapping_add(1);
            }
        }
    }

    dest as u64 + header.entry_point
}

#[repr(C)]
struct Header {
    magic: u32,
    bits: u8,
    endian: u8,
    version: u8,
    abi: u8,
    padding: [u8; 8],
    object_type: u16,
    machine_type: u16,
    x_version: u32,
    entry_point: u64,
    ph_offset: u64,
    sh_offset: u64,
    flags: u32,
    header_size: u16,
    ph_entry_size: u16,
    ph_entry_count: u16,
    sh_entry_size: u16,
    sh_entry_count: u16,
    name_string_table_index: u16,
}

#[repr(C)]
struct SegmentHeader {
    seg_type: u32,
    flags: u32,
    data_offset: u64,
    virt_addr: u64,
    unused: u64,
    data_size: u64,
    virt_size: u64,
    alignment: u64,
}

const SEGTYPE_LOAD: u32 = 1;
const SEGTYPE_DYNAMIC: u32 = 2;

#[repr(C)]
struct DynamicEntry {
    tag: i64,
    value: u64,
}

const DE_TAG_RELA: i64 = 7;
const DE_TAG_RELASZ: i64 = 8;

#[repr(C)]
struct RelA {
    addr: u64,
    info: u64,
    addend: i64,
}

const R_RELATIVE: u32 = 8;

#[cfg(debug_assertions)]
#[repr(C)]
struct SectionHeader {
    name_offset: u32,
    sec_type: u32,
    flags: u64,
    virt_addr: u64,
    file_offset: u64,
    size: u64,
    link: u32,
    info: u32,
    alignment: u64,
    entry_size: u64,
}

#[cfg(debug_assertions)]
const SHT_PROGBITS: u32 = 1;
