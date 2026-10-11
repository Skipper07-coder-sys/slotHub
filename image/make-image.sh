#!/usr/bin/env bash
# Bake a slot tree into a BaseOS image so one flash gives a ready card.
#
# BaseOS reformats its FAT data partition (p7) on first boot, so anything
# copied there would be wiped. Instead the tree goes into the persistent
# ext4 /data partition (p6) as a tar, and expand-storage on the rootfs (p5)
# is taught to unpack it onto the fresh card right after formatting.
#
# usage: make-image.sh BASEOS_IMG SLOT_TREE DEFAULTS_DIR OUT_IMG
set -euo pipefail

base=$1 tree=$2 defaults=$3 out=$4
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

cp "$base" "$out"

# start/size (sectors) of a GPT partition, by name
part() {
	python3 - "$out" "$1" <<'PY'
import struct, sys
f = open(sys.argv[1], 'rb')
f.seek(512)
hdr = f.read(92)
lba, n, size = struct.unpack('<QII', hdr[72:88])
f.seek(lba * 512)
for _ in range(n):
    e = f.read(size)
    first, last = struct.unpack('<QQ', e[32:48])
    if e[56:128].decode('utf-16le').rstrip('\0') == sys.argv[2]:
        print(first, last - first + 1)
        break
else:
    sys.exit('no partition ' + sys.argv[2])
PY
}

read -r p5_start p5_len < <(part rootfs)
read -r p6_start p6_len < <(part UDISK)
dd if="$out" of="$work/p5.img" bs=512 skip="$p5_start" count="$p5_len" status=none
dd if="$out" of="$work/p6.img" bs=512 skip="$p6_start" count="$p6_len" status=none

# What lands on the card: the slot tree plus the chosen defaults.
mkdir -p "$work/card"
cp -R "$tree"/. "$work/card/"
if find "$work/card" -type l | grep -q .; then
	echo "symlinks cannot go on the FAT card" >&2
	exit 1
fi
mkdir -p "$work/card/Config"
cp "$defaults/slot.state" "$work/card/Config/slot.state"
[ -f "$defaults/baseos.conf" ] && cp "$defaults/baseos.conf" "$work/card/baseos.conf"
tar -C "$work/card" --owner=0 --group=0 --format=gnu -cf "$work/preload.tar" .

debugfs -w -R "write $work/preload.tar /slot-preload.tar" "$work/p6.img"

debugfs -R "cat /usr/sbin/expand-storage" "$work/p5.img" > "$work/expand-storage"
grep -q 'baseos.conf "$dst/baseos.conf"' "$work/expand-storage" ||
	{ echo "expand-storage changed upstream; update make-image.sh" >&2; exit 1; }
sed -i '/baseos.conf "$dst\/baseos.conf"/a\
	if [ -f /data/slot-preload.tar ] \&\& tar -xof /data/slot-preload.tar -C "$dst"; then\
		rm -f /data/slot-preload.tar\
		log "slot unpacked onto the card"\
	fi' "$work/expand-storage"
debugfs -w "$work/p5.img" <<CMDS
rm /usr/sbin/expand-storage
write $work/expand-storage /usr/sbin/expand-storage
set_inode_field /usr/sbin/expand-storage mode 0100755
set_inode_field /usr/sbin/expand-storage uid 0
set_inode_field /usr/sbin/expand-storage gid 0
CMDS

e2fsck -fn "$work/p5.img"
e2fsck -fn "$work/p6.img"
debugfs -R "cat /usr/sbin/expand-storage" "$work/p5.img" | grep -q slot-preload
debugfs -R "stat /slot-preload.tar" "$work/p6.img" | grep -q 'Type: regular'

dd if="$work/p5.img" of="$out" bs=512 seek="$p5_start" conv=notrunc status=none
dd if="$work/p6.img" of="$out" bs=512 seek="$p6_start" conv=notrunc status=none
echo "wrote $out"
