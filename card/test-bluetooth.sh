#!/bin/sh
set -eu

HERE="$(cd "$(dirname "$0")" && pwd)"
SCRIPT="$HERE/System/bluetooth.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT HUP INT TERM
FAILS=0

fail() {
	echo "FAIL: $*"
	FAILS=$((FAILS + 1))
}

# A fake bluetoothctl that answers from files under $TMP/bt: one directory per
# device seen, holding its info text.
reset() {
	rm -rf "$TMP/bin" "$TMP/run" "$TMP/bt" "$TMP/hci0" "$TMP"/*.log
	mkdir -p "$TMP/bin" "$TMP/run" "$TMP/bt"
	cat > "$TMP/bin/bluetoothctl" <<FAKE
#!/bin/sh
echo "\$*" >> "$TMP/btctl.log"
case "\$1" in
	"") cat > /dev/null ;;
	show) echo "Powered: yes" ;;
	devices|paired-devices)
		for d in "$TMP"/bt/*; do
			[ -d "\$d" ] || continue
			[ "\$1" = devices ] || grep -q "Paired: yes" "\$d/info" || continue
			echo "Device \$(basename "\$d") \$(sed -n 's/^.*Name: //p' "\$d/info")"
		done ;;
	info) cat "$TMP/bt/\$2/info" 2>/dev/null ;;
	pair) echo "	Paired: yes" >> "$TMP/bt/\$2/info" ;;
	connect) echo "	Connected: yes" >> "$TMP/bt/\$2/info" ;;
esac
exit 0
FAKE
	cat > "$TMP/bin/bluealsa-aplay" <<FAKE
#!/bin/sh
for d in "$TMP"/bt/*; do
	grep -q "Connected: yes" "\$d/info" 2>/dev/null || continue
	grep -q "Class:" "\$d/info" || continue
	echo "bluealsa:DEV=\$(basename "\$d"),PROFILE=a2dp,SRV=org.bluealsa"
done
FAKE
	cat > "$TMP/bin/pidof" <<FAKE
#!/bin/sh
[ -e "$TMP/run/\$1" ]
FAKE
	cat > "$TMP/bin/bluealsa" <<FAKE
#!/bin/sh
echo "\$*" >> "$TMP/bluealsa.log"
: > "$TMP/run/bluealsa"
FAKE
	cat > "$TMP/bin/systemctl" <<FAKE
#!/bin/sh
echo "\$*" >> "$TMP/systemctl.log"
case "\$1" in
	is-active) [ -e "$TMP/run/bluetoothd" ] ;;
	start|restart) : > "$TMP/run/bluetoothd" ;;
	stop) rm -f "$TMP/run/bluetoothd" ;;
esac
FAKE
	cat > "$TMP/vendor.sh" <<FAKE
#!/bin/sh
echo "\$*" >> "$TMP/vendor.log"
[ "\$1" = init ] && mkdir -p "$TMP/hci0"
exit 0
FAKE
	for f in rfkill killall hciconfig; do
		printf '#!/bin/sh\nexit 0\n' > "$TMP/bin/$f"
	done
	chmod 755 "$TMP"/bin/*
}

device() {
	mac="$1"
	shift
	mkdir -p "$TMP/bt/$mac"
	for line in "$@"; do printf '\t%s\n' "$line"; done > "$TMP/bt/$mac/info"
}

run() {
	PATH="$TMP/bin:$PATH" \
		AGS_BT_LOG="$TMP/bt.log" AGS_BT_HCI="$TMP/hci0" AGS_BT_VENDOR="$TMP/vendor.sh" \
		AGS_BT_VENDOR_LOCK="$TMP/init_bt" AGS_BLUEALSA="$TMP/bin/bluealsa" AGS_BT_SCAN_S=0 \
		AGS_BLUEALSA_APLAY="$TMP/bin/bluealsa-aplay" \
		sh "$SCRIPT" "$@" > /dev/null 2>&1
}

reset
run on && rc=0 || rc=$?
[ "$rc" = 0 ] || fail "on exited $rc"
[ -d "$TMP/hci0" ] || fail "on never attached the controller"
grep -q "a2dp-source" "$TMP/bluealsa.log" 2>/dev/null || fail "on never started bluealsa as an A2DP source"
grep -q "power on" "$TMP/btctl.log" || fail "on never powered the adapter"

reset
device AA:AA:AA:AA:AA:01 "Name: Keyboard" "Icon: input-keyboard" "RSSI: -20"
device AA:AA:AA:AA:AA:02 "Name: Far Speaker" "Class: 0x240414" "Icon: audio-card" "RSSI: -80"
device AA:AA:AA:AA:AA:03 "Name: AirPods Pro" "Class: 0x240418" "Icon: audio-headphones" "RSSI: -40"
device AA:AA:AA:AA:AA:04 "Name: AirPods Pro - Find My" "Icon: audio-headphones" "UUID: Audio Sink" "RSSI: -30"
run pair && rc=0 || rc=$?
[ "$rc" = 0 ] || fail "pair exited $rc"
grep -q "^pair AA:AA:AA:AA:AA:03" "$TMP/btctl.log" || fail "pair did not pick the nearest headphones"
grep -q "^trust AA:AA:AA:AA:AA:03" "$TMP/btctl.log" || fail "pair did not trust them"
grep -q "Connected: yes" "$TMP/bt/AA:AA:AA:AA:AA:03/info" || fail "pair did not connect them"
grep -q "^pair AA:AA:AA:AA:AA:01" "$TMP/btctl.log" && fail "pair paired a keyboard"
grep -q "^pair AA:AA:AA:AA:AA:04" "$TMP/btctl.log" && fail "pair paired the LE Find My entry, which carries no audio"

reset
device AA:AA:AA:AA:AA:01 "Name: Keyboard" "Icon: input-keyboard" "RSSI: -20"
run pair && rc=0 || rc=$?
[ "$rc" = 3 ] || fail "pair with no headphones exited $rc, not 3"

reset
device AA:AA:AA:AA:AA:03 "Alias: Le battonetes 2 - Find My" "Class: 0x240418" "Icon: audio-headphones" "Paired: yes"
run on && rc=0 || rc=$?
grep -q "^connect AA:AA:AA:AA:AA:03" "$TMP/btctl.log" || fail "on did not reconnect paired headphones"
out="$(PATH="$TMP/bin:$PATH" AGS_BT_LOG="$TMP/bt.log" AGS_BLUEALSA_APLAY="$TMP/bin/bluealsa-aplay" sh "$SCRIPT" status 2>/dev/null)" || true
[ "$out" = "AA:AA:AA:AA:AA:03 Le battonetes 2" ] || fail "status printed '$out'"

reset
device AA:AA:AA:AA:AA:03 "Name: AirPods Pro" "Class: 0x240418" "Paired: yes" "Connected: yes"
rm -f "$TMP/bin/bluealsa-aplay"
printf '#!/bin/sh\nexit 0\n' > "$TMP/bin/bluealsa-aplay"
chmod 755 "$TMP/bin/bluealsa-aplay"
PATH="$TMP/bin:$PATH" AGS_BT_LOG="$TMP/bt.log" AGS_BLUEALSA_APLAY="$TMP/bin/bluealsa-aplay" \
	sh "$SCRIPT" status > /dev/null 2>&1 && fail "status called a link with no A2DP stream playable"

reset
: > "$TMP/run/bluetoothd"
run off && rc=0 || rc=$?
[ "$rc" = 0 ] || fail "off exited $rc"
[ -e "$TMP/run/bluetoothd" ] && fail "off left bluetoothd running"

if [ "$FAILS" -gt 0 ]; then
	echo "$FAILS bluetooth.sh check(s) failed"
	exit 1
fi
echo "bluetooth.sh: ok"
