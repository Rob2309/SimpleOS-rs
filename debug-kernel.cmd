
set architecture i386:x86-64
target remote tcp::26000

watch {unsigned long long}0x1000
c

set $ktext = {unsigned long long}0x1000
add-symbol-file target/kernel.dbg $ktext

layout split

b _start
c
