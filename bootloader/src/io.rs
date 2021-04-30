use uefi::{proto::media::file::{FileAttribute, FileInfo, FileMode, FileType}, table::{Boot, SystemTable, boot::MemoryType}};
use uefi::proto::media::file::File;

use crate::allocator;

pub struct FileData {
    pub size: u64,
    pub data: *mut u8,
}

pub fn read_file(system_table: &SystemTable<Boot>, path: &str) -> FileData {
    let mut volume;
    unsafe {
        let fs = &mut *super::FILESYSTEM;
        volume = fs.open_volume().expect("Failed to open FileSystem root").split().1;
    }

    let mut file = volume.open(path, FileMode::Read, FileAttribute::empty()).expect("Failed to open file").split().1;

    let size;
    {
        let mut info_buf = [0u8; 1024];
        let info = file.get_info::<FileInfo>(&mut info_buf).expect("Failed to get file info").split().1;
        size = info.file_size();
    }

    let buffer = allocator::allocate(system_table, size as usize, MemoryType::LOADER_DATA);

    match file.into_type().expect("Not a file").split().1 {
        FileType::Regular(mut file) => {
            let _ = file.read(unsafe{core::slice::from_raw_parts_mut(buffer, size as usize)}).expect("Failed to read file");
        }
        _ => panic!("Not a file")
    }

    FileData {
        size,
        data: buffer,
    }
}
