
# Path to a folder containing OVMF_CODE.fd and OVMF_VARS.fd
# (only needed for 'make debug-kernel')
ovmf_dir:=/usr/share/OVMF

.PHONY: run
run:
	cargo osbuild
	rm -rf target/vm.vdi
	VBoxManage convertfromraw target/image/x86_64/debug/image.img target/vm.vdi --format VDI --uuid 430eee2a-0fdf-4d2a-88f0-5b99ea8cffcb
	VBoxManage startvm SimpleOS-rs

.PHONY: run-release
run-release:
	cargo osbuild --release
	rm -rf target/vm.vdi
	VBoxManage convertfromraw target/image/x86_64/release/image.img target/vm.vdi --format VDI --uuid 430eee2a-0fdf-4d2a-88f0-5b99ea8cffcb
	VBoxManage startvm SimpleOS-rs

.PHONY: debug-kernel
debug-kernel:
	cargo osbuild
	cp target/kernel-x86_64/debug/kernel target/kernel.dbg
	qemu-system-x86_64 -gdb tcp::26000 -m 4096 -machine q35 -cpu qemu64 -net none -drive if=pflash,unit=0,format=raw,file=$(ovmf_dir)/OVMF_CODE.fd,readonly=on -drive if=pflash,unit=1,format=raw,file=$(ovmf_dir)/OVMF_VARS.fd,readonly=on -drive file=target/image/x86_64/debug/image.img,if=ide -S & \
	gdb --command=debug-kernel.cmd
	