
# === Default configuration values ===
# should either be Debug or Release
config:=Debug

# Path to a folder containing OVMF_CODE.fd and OVMF_VARS.fd
# (only needed for 'make debug-kernel')
ovmf_dir:=/usr/share/OVMF


ifeq ($(config),Debug)
CARGO_CFG_FLAG := 
TARGET_FOLDER:=debug
endif
ifeq ($(config),Release)
CARGO_CFG_FLAG := --release
TARGET_FOLDER:=release
endif

target/image/$(TARGET_FOLDER)/image.vdi: target/image/$(TARGET_FOLDER)/image.img
	@ mkdir -p $(dir $@)
	rm -rf $@
	VBoxManage convertfromraw $< $@ --format VDI --uuid 430eee2a-0fdf-4d2a-88f0-5b99ea8cffcb

.PHONY: run
run: target/image/$(TARGET_FOLDER)/image.vdi
	cp $< target/image/image.vdi
	VBoxManage startvm SimpleOS-rs

.PHONY: debug-kernel
debug-kernel: target/image/$(TARGET_FOLDER)/image.img
	cp target/image/$(TARGET_FOLDER)/kernel.sys target/image/kernel.dbg
	qemu-system-x86_64 -gdb tcp::26000 -m 4096 -machine q35 -cpu qemu64 -net none -drive if=pflash,unit=0,format=raw,file=$(ovmf_dir)/OVMF_CODE.fd,readonly=on -drive if=pflash,unit=1,format=raw,file=$(ovmf_dir)/OVMF_VARS.fd,readonly=on -drive file=$<,if=ide -S & \
	gdb --command=debug-kernel.cmd

target/image/$(TARGET_FOLDER)/image.img: target/image/$(TARGET_FOLDER)/partition.img
	@ mkdir -p $(dir $@)
	dd if=/dev/zero of=$@ bs=512 count=110000
	parted $@ mklabel gpt mkpart SimpleOS-rs fat32 2048s 104447s toggle 1 esp
	dd if=$< of=$@ bs=512 conv=notrunc seek=2048

target/image/$(TARGET_FOLDER)/partition.img: target/image/$(TARGET_FOLDER)/BOOTX64.EFI target/image/$(TARGET_FOLDER)/kernel.sys
	@ mkdir -p $(dir $@)
	dd if=/dev/zero of=$@ bs=512 count=102400
	mkfs.fat $@
	mmd -i $@ ::/EFI
	mmd -i $@ ::/EFI/BOOT
	mcopy -i $@ $^ ::/EFI/BOOT

target/image/$(TARGET_FOLDER)/BOOTX64.EFI: target/x86_64-unknown-uefi/$(TARGET_FOLDER)/bootloader.efi
	@ mkdir -p $(dir $@)
	cp $< $@

target/image/$(TARGET_FOLDER)/kernel.sys: target/x86_64-unknown-none/$(TARGET_FOLDER)/kernel
	@ mkdir -p $(dir $@)
	cp $< $@

.PHONY: target/x86_64-unknown-none/$(TARGET_FOLDER)/kernel
target/x86_64-unknown-none/$(TARGET_FOLDER)/kernel: 
	cd kernel && cargo build $(CARGO_CFG_FLAG)

.PHONY: target/x86_64-unknown-uefi/$(TARGET_FOLDER)/bootloader.efi
target/x86_64-unknown-uefi/$(TARGET_FOLDER)/bootloader.efi:
	cd bootloader && cargo build $(CARGO_CFG_FLAG)

.PHONY: clean
clean: 
	cargo clean
	