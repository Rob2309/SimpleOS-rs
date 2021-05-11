use std::{env, fs, io::{self, Seek}, process::{Command, exit}};

const CARGO: &str = env!("CARGO");
const ROOT_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");

fn print_usage() {
    println!("Usage: cargo osbuild [--target=TARGET] [--release]");
}

fn main() {
    let mut arch = "x86_64".to_owned();
    let mut release_mode = false;
    let mut clippy_mode = false;

    for arg in env::args() {
        if let Some(a) = arg.strip_prefix("--target=") {
            arch = a.to_owned();
        } else if arg == "--release" {
            release_mode = true;
        } else if arg == "--clippy" {
            clippy_mode = true;  
        } else if arg == "--help" || arg == "-h" {
            print_usage();
            exit(0);
        }
    }

    if clippy_mode {
        run_clippy(arch);
    } else {
        build(arch, release_mode);
    }
}

fn run_clippy(arch: String) {
    println!("-- Clippy bootloader");
    {
        let bootloader_target = format!("{}-unknown-uefi", &arch);
        let mut command = Command::new(CARGO);
        command.arg("clippy").arg("-p").arg("bootloader")
            .arg("-Zbuild-std=core,compiler_builtins")
            .arg("-Zbuild-std-features=compiler-builtins-mem")
            .arg(format!("--target={}", &bootloader_target));
        command.status().unwrap();
    }
    
    println!("-- Clippy kernel");
    {
        let kernel_target = format!("kernel-{}.json", &arch);
        let mut command = Command::new(CARGO);
        command.arg("clippy").arg("-p").arg("kernel")
            .arg("-Zbuild-std=core,compiler_builtins")
            .arg("-Zbuild-std-features=compiler-builtins-mem")
            .arg(format!("--target={}/{}", ROOT_DIR, &kernel_target));
        command.status().unwrap();
    }
}

fn build(arch: String, release_mode: bool) {
    let profile_name = if release_mode { "release" } else { "debug" };

    println!("-- Building for {}", arch);

    println!("-- Building bootloader ({})", profile_name);
    let bootloader_target = format!("{}-unknown-uefi", &arch);
    let status = {
        let mut command = Command::new(CARGO);
        command.arg("build").arg("-p").arg("bootloader")
            .arg("-Zbuild-std=core,compiler_builtins")
            .arg("-Zbuild-std-features=compiler-builtins-mem")
            .arg(format!("--target={}", &bootloader_target));
        if release_mode {
            command.arg("--release");
        }

        command.status().unwrap()
    };
    assert!(status.success(), "Failed to build bootloader");

    println!("-- Building kernel ({})", profile_name);
    let kernel_target = format!("kernel-{}.json", &arch);
    let status = {
        let mut command = Command::new(CARGO);
        command.arg("build").arg("-p").arg("kernel")
            .arg("-Zbuild-std=core,compiler_builtins")
            .arg("-Zbuild-std-features=compiler-builtins-mem")
            .arg(format!("--target={}/{}", ROOT_DIR, &kernel_target));
        if release_mode {
            command.arg("--release");
        }
        
        command.status().unwrap()
    };
    assert!(status.success(), "Failed to build kernel");

    println!("-- Building efi partition");
    const MB: u64 = 1024 * 1024;

    let bootloader_path = format!("{}/target/{}/{}/bootloader.efi", ROOT_DIR, &bootloader_target, &profile_name);
    let kernel_path = format!("{}/target/kernel-{}/{}/kernel", ROOT_DIR, &arch, &profile_name);
    let image_dir = format!("{}/target/image/{}/{}", ROOT_DIR, &arch, &profile_name);
    let partition_path = format!("{}/partition.img", &image_dir);

    fs::create_dir_all(&image_dir).unwrap();

    let bootloader_size = fs::metadata(&bootloader_path).unwrap().len();
    let kernel_size = fs::metadata(&kernel_path).unwrap().len();
    let partition_size = MB + (bootloader_size + kernel_size + MB - 1) / MB * MB;
    
    {
        let mut partition_file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&partition_path)
            .unwrap();
        partition_file.set_len(partition_size).unwrap();

        fatfs::format_volume(&partition_file, fatfs::FormatVolumeOptions::new().volume_label(*b"SimpleOS-rs")).unwrap();

        partition_file.seek(io::SeekFrom::Start(0)).unwrap();
        let partition = fatfs::FileSystem::new(&partition_file, fatfs::FsOptions::new()).unwrap();

        partition.root_dir().create_dir("EFI").unwrap();
        partition.root_dir().create_dir("EFI/BOOT").unwrap();

        let mut bootloader_out = partition.root_dir().create_file("EFI/BOOT/BOOTX64.EFI").unwrap();
        let mut bootloader_in = fs::File::open(&bootloader_path).unwrap();
        io::copy(&mut bootloader_in, &mut bootloader_out).unwrap();

        let mut kernel_out = partition.root_dir().create_file("EFI/BOOT/kernel.sys").unwrap();
        let mut kernel_in = fs::File::open(&kernel_path).unwrap();
        io::copy(&mut kernel_in, &mut kernel_out).unwrap();
    }

    println!("-- Building system image");
    let image_path = format!("{}/image.img", &image_dir);
    let image_size = MB + partition_size;

    {
        let mut image_file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&image_path)
            .unwrap();
        image_file.set_len(image_size).unwrap();

        gpt::mbr::ProtectiveMBR::new().overwrite_lba0(&mut image_file).unwrap();

        let mut image = gpt::GptConfig::new()
            .writable(true)
            .logical_block_size(gpt::disk::LogicalBlockSize::Lb512)
            .initialized(false)
            .create_from_device(Box::new(&mut image_file), None).unwrap();
        image.update_partitions(Default::default()).unwrap();
        
        let part_id = image.add_partition("boot", partition_size, gpt::partition_types::EFI, 0).unwrap();
        let part = image.partitions().get(&part_id).unwrap();
        let part_offset = part.bytes_start(gpt::disk::LogicalBlockSize::Lb512).unwrap();
        image.write().unwrap();

        image_file.seek(io::SeekFrom::Start(part_offset)).unwrap();
        io::copy(&mut fs::File::open(&partition_path).unwrap(), &mut image_file).unwrap();
    }

    println!("-- Finished");
}
