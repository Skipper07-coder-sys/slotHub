#!/bin/sh
set -eu

HERE="$(cd "$(dirname "$0")" && pwd)"
SCRIPT="$HERE/System/bluetooth.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT HUP INT TERM
FAILS=0
TAB="$(printf '\t')"

fail() {
	echo "FAIL: $*"
	FAILS=$((FAILS + 1))
}

# A fake bluetoothctl that answers from files under $TMP/bt: one directory per
# device seen, holding its info text. "A2DP: yes" in an info stands for a BlueZ
# media transport, which the fake dbus-send lists.
reset() {
	rm -rf "$TMP/bin" "$TMP/run" "$TMP/bt" "$TMP/hci0" "$TMP"/*.log "$TMP/bluetooth.conf" "$TMP/status"
	mkdir -p "$TMP/bin" "$TMP/run" "$TMP/bt"
	cat > "$TMP/bin/bluetoothctl" <<FAKE
#!/bin/sh
echo "\$*" >> "$TMP/btctl.log"
# BlueZ 5.66 dropped paired-devices; devices Paired replaced it.
if [ "\$1" = paired-devices ] && [ -n "\${FAKE_NEW_BLUEZ:-}" ]; then
	echo "Invalid command in menu main: paired-devices"
	exit 0
fi
case "\$1" in
	"") cat >> "$TMP/btctl.log" ;;
	show) echo "Controller 68:8F:C9:35:51:EB slot [default]"; echo "	Powered: yes" ;;
	devices|paired-devices)
		for d in "$TMP"/bt/*; do
			[ -d "\$d" ] || continue
			[ "\$1 \${2:-}" = "devices " ] || grep -q "Paired: yes" "\$d/info" || continue
			echo "Device \$(basename "\$d") \$(sed -n 's/^.*Name: //p' "\$d/info")"
		done ;;
	info) cat "$TMP/bt/\$2/info" 2>/dev/null ;;
	pair)
		if [ -n "\${FAKE_REJECT:-}" ]; then
			echo "Failed to pair: org.bluez.Error.AuthenticationRejected"
		else
			echo "	Paired: yes" >> "$TMP/bt/\$2/info"
		fi ;;
	connect)
		if grep -q "0000110b-" "$TMP/bt/\$2/info"; then
			printf '\tConnected: yes\n\tA2DP: yes\n' >> "$TMP/bt/\$2/info"
		else
			echo "Failed to connect: org.bluez.Error.Failed br-connection-profile-unavailable"
		fi ;;
esac
exit 0
FAKE
	cat > "$TMP/bin/dbus-send" <<FAKE
#!/bin/sh
echo "\$*" >> "$TMP/dbus.log"
case "\$*" in
	*GetManagedObjects*)
		for d in "$TMP"/bt/*; do
			grep -q "A2DP: yes" "\$d/info" 2>/dev/null || continue
			echo "object path \"/org/bluez/hci0/dev_\$(basename "\$d" | tr ':' '_')/sep1/fd0\""
		done ;;
	*Properties.Get*Volume*) echo "variant uint16 \${FAKE_VOLUME:-64}" ;;
esac
FAKE
	cat > "$TMP/bin/pidof" <<FAKE
#!/bin/sh
[ -e "$TMP/run/\$1" ]
FAKE
	cat > "$TMP/bin/bluealsa" <<FAKE
#!/bin/sh
echo "\$*" >> "$TMP/bluealsa.log"
case "\$*" in
	*--keep-alive=*) [ -n "\${FAKE_OLD_BLUEALSA:-}" ] && exit 1 ;;
esac
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

SINK="UUID: Audio Sink                (0000110b-0000-1000-8000-00805f9b34fb)"

run() {
	PATH="$TMP/bin:$PATH" \
		AGS_BT_LOG="$TMP/bt.log" AGS_BT_CONF="$TMP/bluetooth.conf" AGS_BT_HCI="$TMP/hci0" \
		AGS_BT_VENDOR="$TMP/vendor.sh" AGS_BT_VENDOR_LOCK="$TMP/init_bt" \
		AGS_BLUEALSA="$TMP/bin/bluealsa" AGS_BT_SCAN_S=0 \
		AGS_DBUS_SEND="$TMP/bin/dbus-send" AGS_BT_STATUS_SEEN="$TMP/status" \
		sh "$SCRIPT" "$@"
}

quiet() {
	run "$@" > /dev/null 2>&1
}

reset
quiet on && rc=0 || rc=$?
[ "$rc" = 0 ] || fail "on exited $rc"
[ -d "$TMP/hci0" ] || fail "on never attached the controller"
grep -q "a2dp-source" "$TMP/bluealsa.log" 2>/dev/null || fail "on never started bluealsa as an A2DP source"
grep -q -- "--keep-alive=" "$TMP/bluealsa.log" || fail "on did not ask bluealsa to keep the stream alive"
grep -q "power on" "$TMP/btctl.log" || fail "on never powered the adapter"
grep -q "^--- debug" "$TMP/bt.log" || fail "on did not write the debug dump"

# An older bluealsa rejects --keep-alive: the next spelling is tried.
reset
FAKE_OLD_BLUEALSA=1 quiet on && rc=0 || rc=$?
[ "$rc" = 0 ] || fail "on with an old bluealsa exited $rc"
grep -q -- "--a2dp-keep-alive=" "$TMP/bluealsa.log" || fail "on did not fall back to the old keep-alive spelling"

# Scan lists only devices that can play audio, nearest first, in both RSSI
# spellings bluetoothctl uses. A TV with Audio Source only is not offered.
reset
device AA:AA:AA:AA:AA:01 "Name: Keyboard" "Icon: input-keyboard" "RSSI: -20"
device AA:AA:AA:AA:AA:02 "Name: Far Speaker" "Class: 0x240414" "Icon: audio-card" "$SINK" "RSSI: 0xffffffb0 (-80)"
device AA:AA:AA:AA:AA:03 "Name: AirPods Pro" "Class: 0x240418" "Icon: audio-headphones" "$SINK" "RSSI: -40"
device AA:AA:AA:AA:AA:04 "Name: AirPods Pro - Find My" "Icon: audio-headphones" "RSSI: -30"
device AA:AA:AA:AA:AA:05 "Name: 43PUS8909/12" "Class: 0x3c0000" "Icon: audio-card" "UUID: Audio Source (0000110a-0000-1000-8000-00805f9b34fb)" "RSSI: -35"
out="$(run scan 2>/dev/null)" && rc=0 || rc=$?
[ "$rc" = 0 ] || fail "scan exited $rc"
want="-40${TAB}0${TAB}AA:AA:AA:AA:AA:03${TAB}AirPods Pro
-80${TAB}0${TAB}AA:AA:AA:AA:AA:02${TAB}Far Speaker"
[ "$out" = "$want" ] || fail "scan printed:
$out"
grep -q "^scan on" "$TMP/btctl.log" || fail "scan never searched"

reset
device AA:AA:AA:AA:AA:01 "Name: Keyboard" "Icon: input-keyboard" "RSSI: -20"
out="$(run scan 2>/dev/null)" && rc=0 || rc=$?
[ "$rc" = 0 ] || fail "scan with nothing to play to exited $rc"
[ -z "$out" ] || fail "scan offered a keyboard: '$out'"

# Connect pairs, trusts, asks for the audio profiles and remembers the choice.
reset
device AA:AA:AA:AA:AA:03 "Name: AirPods Pro" "Class: 0x240418" "Icon: audio-headphones" "$SINK" "RSSI: -40"
quiet connect AA:AA:AA:AA:AA:03 && rc=0 || rc=$?
[ "$rc" = 0 ] || fail "connect exited $rc"
grep -q "^pair AA:AA:AA:AA:AA:03" "$TMP/btctl.log" || fail "connect did not pair"
grep -q "^trust AA:AA:AA:AA:AA:03" "$TMP/btctl.log" || fail "connect did not trust"
grep -q "^connect AA:AA:AA:AA:AA:03" "$TMP/btctl.log" || fail "connect never asked for the audio profiles"
grep -q "^mac=AA:AA:AA:AA:AA:03$" "$TMP/bluetooth.conf" || fail "connect did not remember the headphones"
grep -q "^name=AirPods Pro$" "$TMP/bluetooth.conf" || fail "connect did not remember their name"
grep -q "^scan on" "$TMP/btctl.log" && fail "connect searched, which drops A2DP"

# Headphones already bonded get a connect, never a second pairing.
reset
device AA:AA:AA:AA:AA:03 "Name: AirPods Pro" "$SINK" "Paired: yes" "Connected: yes"
quiet connect AA:AA:AA:AA:AA:03 && rc=0 || rc=$?
[ "$rc" = 0 ] || fail "connect on bonded headphones exited $rc"
grep -q "^pair " "$TMP/btctl.log" && fail "connect paired headphones that were already bonded"
grep -q "^connect AA:AA:AA:AA:AA:03" "$TMP/btctl.log" || fail "connect trusted a link with no A2DP and never asked"

# Headphones already playing are left alone.
reset
device AA:AA:AA:AA:AA:03 "Name: AirPods Pro" "$SINK" "Paired: yes" "Connected: yes" "A2DP: yes"
quiet connect AA:AA:AA:AA:AA:03 && rc=0 || rc=$?
[ "$rc" = 0 ] || fail "connect on playing headphones exited $rc"
grep -q "^connect " "$TMP/btctl.log" && fail "connect reconnected headphones that were already playing"

# Silent headphones get a volume; others keep theirs.
reset
device AA:AA:AA:AA:AA:03 "Name: AirPods Pro" "$SINK" "Paired: yes" "Connected: yes" "A2DP: yes"
FAKE_VOLUME=0 quiet connect AA:AA:AA:AA:AA:03
grep -q "Properties.Set.*Volume variant:uint16:90" "$TMP/dbus.log" || fail "connect left silent headphones at volume 0"
reset
device AA:AA:AA:AA:AA:03 "Name: AirPods Pro" "$SINK" "Paired: yes" "Connected: yes" "A2DP: yes"
quiet connect AA:AA:AA:AA:AA:03
grep -q "Properties.Set" "$TMP/dbus.log" && fail "connect changed a volume that was not zero"

reset
device AA:AA:AA:AA:AA:03 "Name: AirPods Pro" "Class: 0x240418" "$SINK" "RSSI: -40"
FAKE_REJECT=1 quiet connect AA:AA:AA:AA:AA:03 && rc=0 || rc=$?
[ "$rc" = 4 ] || fail "a refused pairing exited $rc, not 4"
grep -q "^remove AA:AA:AA:AA:AA:03" "$TMP/btctl.log" || fail "a refused pairing did not forget the stale bond"
[ -e "$TMP/bluetooth.conf" ] && fail "a refused pairing was remembered"

# A device without a sink pairs but cannot connect: that fails with a dump.
reset
device AA:AA:AA:AA:AA:05 "Name: 43PUS8909/12" "Icon: audio-card" "UUID: Audio Source (0000110a-0000-1000-8000-00805f9b34fb)"
quiet connect AA:AA:AA:AA:AA:05 && rc=0 || rc=$?
[ "$rc" = 1 ] || fail "connect to a TV exited $rc, not 1"
grep -q "^--- debug" "$TMP/bt.log" || fail "a failed connect did not dump the debug state"

# On reconnects the remembered headphones even when BlueZ lists nothing as paired.
reset
device AA:AA:AA:AA:AA:03 "Alias: Le battonetes 2 - Find My" "$SINK" "Paired: yes"
printf 'mac=AA:AA:AA:AA:AA:03\nname=Le battonetes 2\n' > "$TMP/bluetooth.conf"
FAKE_NEW_BLUEZ=1 quiet on && rc=0 || rc=$?
[ "$rc" = 0 ] || fail "on with remembered headphones exited $rc"
grep -q "^connect AA:AA:AA:AA:AA:03" "$TMP/btctl.log" || fail "on did not reconnect the remembered headphones"
out="$(run status 2>/dev/null)" || true
[ "$out" = "AA:AA:AA:AA:AA:03 Le battonetes 2" ] || fail "status printed '$out'"
grep -q "status: AA:AA:AA:AA:AA:03" "$TMP/bt.log" || fail "status did not log what it found"

# Status follows the A2DP transport alone; a mere link is not Connected.
reset
device AA:AA:AA:AA:AA:03 "Name: AirPods Pro" "$SINK" "Paired: yes" "Connected: yes"
out="$(run status 2>/dev/null)" || true
[ -z "$out" ] || fail "status called linked headphones with no A2DP stream connected: '$out'"

reset
: > "$TMP/run/bluetoothd"
mkdir -p "$TMP/hci0"
quiet off && rc=0 || rc=$?
[ "$rc" = 0 ] || fail "off exited $rc"
grep -q "^power off" "$TMP/btctl.log" || fail "off did not power the adapter down"
grep -q "stop" "$TMP/systemctl.log" 2>/dev/null && fail "off stopped bluetoothd, which forces a controller re-attach"
[ -d "$TMP/hci0" ] || fail "off detached the controller"

if [ "$FAILS" -gt 0 ]; then
	echo "$FAILS bluetooth.sh check(s) failed"
	exit 1
fi
echo "bluetooth.sh: ok"
