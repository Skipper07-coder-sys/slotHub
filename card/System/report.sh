#!/bin/sh
# Diagnostics for slot: a hardware self-test and a bug report folder on the
# card. Verbs:
#   selftest         check the card, the System files, the screen, sound,
#                    buttons, battery and (when slot has them on) Wi-Fi and
#                    Bluetooth; writes selftest.log at the card root and prints
#                    "problems N" last
#   selftest-once    the self-test, only when this slot version has not had
#                    one yet (slot runs it after each update); prints nothing
#                    when there is nothing to do
#   bugreport [why]  run the self-test, then copy the logs, crash notes and a
#                    snapshot of the system into Reports/<time>/ on the card;
#                    prints "saved <folder name>"
#   crashcheck       run by launch_frontend.sh before each start: when the run
#                    before ended in a crash, save a bug report for it and leave
#                    crash/caught for slot to find
# slot passes SLOT_WIFI=1 and SLOT_BT=1 when those radios are meant to be on.
# Exit 0 on success, 3 when the self-test found problems, 1 otherwise.
# Nothing secret goes in a report: no Wi-Fi passwords, no API keys, no saves.
set -u

SD="${SLOT_ROOT:-/mnt/sdcard}"
SYS="$SD/System"
REPORTS="${AGS_REPORTS:-$SD/Reports}"
SELFTEST_LOG="${AGS_SELFTEST_LOG:-$SD/selftest.log}"
PSY="${AGS_PSY:-/sys/class/power_supply/axp2202-battery}"
NET="${AGS_NET_SYS:-/sys/class/net}"
HCI="${AGS_BT_HCI:-/sys/class/bluetooth/hci0}"
FB="${AGS_FB:-/dev/fb0}"
INPUT="${AGS_INPUT:-/dev/input}"
ASOUND="${AGS_ASOUND:-/proc/asound/cards}"
SHA256SUM="${AGS_SHA256SUM:-sha256sum}"
MIN_FREE_KB="${AGS_MIN_FREE_KB:-102400}"
KEEP="${AGS_REPORTS_KEEP:-5}"

stamp() {
	date '+%Y-%m-%d_%H-%M-%S'
}

# Self-test lines are "ok <what>" or "FAIL <what>: <why>".
PROBLEMS=0
ok() {
	echo "ok $*" >> "$SELFTEST_LOG.tmp"
}
bad() {
	echo "FAIL $*" >> "$SELFTEST_LOG.tmp"
	PROBLEMS=$((PROBLEMS + 1))
}

free_kb() {
	df -k "$1" 2>/dev/null | awk 'NR == 2 { print $4 }'
}

selftest() {
	PROBLEMS=0
	: > "$SELFTEST_LOG.tmp" 2>/dev/null || return 1
	echo "self-test $(date '+%F %T') slot $(release)" >> "$SELFTEST_LOG.tmp"

	probe="$SD/.selftest-probe"
	if echo probe > "$probe" 2>/dev/null && [ "$(cat "$probe" 2>/dev/null)" = probe ]; then
		ok "card writable"
	else
		bad "card writable: could not write and read back a file"
	fi
	rm -f "$probe" 2>/dev/null

	kb="$(free_kb "$SD")"
	if [ -z "$kb" ]; then
		bad "card space: df gave nothing"
	elif [ "$kb" -lt "$MIN_FREE_KB" ]; then
		bad "card space: only $((kb / 1024)) MB free"
	else
		ok "card space: $((kb / 1024)) MB free"
	fi

	# The release build lists every file it ships with its checksum.
	if [ -f "$SYS/MANIFEST.sha256" ]; then
		# Low priority: slot may already be running a game.
		nice=""
		command -v nice > /dev/null 2>&1 && nice="nice -n 19"
		bad_files="$(cd "$SYS" && $nice $SHA256SUM -c MANIFEST.sha256 2>/dev/null | grep -v ': OK$')"
		if [ -z "$bad_files" ]; then
			ok "System files match the release"
		else
			bad "System files: $(echo "$bad_files" | wc -l | tr -d ' ') differ from the release"
			echo "$bad_files" | sed 's/^/  /' >> "$SELFTEST_LOG.tmp"
		fi
	else
		ok "System files: no manifest in this build, not checked"
	fi
	for f in slot mgba_libretro.so gpsp_libretro.so; do
		[ -s "$SYS/$f" ] || bad "System/$f is missing"
	done

	if [ -e "$FB" ]; then ok "screen"; else bad "screen: $FB is missing"; fi

	if ls "$INPUT"/event* > /dev/null 2>&1; then
		ok "buttons"
	else
		bad "buttons: no input devices in $INPUT"
	fi

	if grep -q '[0-9] \[' "$ASOUND" 2>/dev/null; then
		ok "sound card"
	else
		bad "sound card: none listed in $ASOUND"
	fi

	cap="$(cat "$PSY/capacity" 2>/dev/null)"
	case "$cap" in
	''|*[!0-9]*) bad "battery: capacity unreadable" ;;
	*) ok "battery: $cap%" ;;
	esac

	if [ "$(date +%Y)" -ge 2024 ] 2>/dev/null; then
		ok "clock: $(date '+%F %T')"
	else
		bad "clock: reads $(date '+%F'), set Date & Time"
	fi

	if [ "${SLOT_WIFI:-0}" = 1 ]; then
		if [ -e "$NET/wlan0" ]; then
			ok "Wi-Fi: wlan0 up"
		else
			bad "Wi-Fi: on in slot but there is no wlan0"
		fi
	fi
	if [ "${SLOT_BT:-0}" = 1 ]; then
		if [ -e "$HCI" ]; then
			ok "Bluetooth: hci0 present"
		else
			bad "Bluetooth: on in slot but there is no hci0"
		fi
	fi

	echo "problems $PROBLEMS" >> "$SELFTEST_LOG.tmp"
	mv -f "$SELFTEST_LOG.tmp" "$SELFTEST_LOG" 2>/dev/null
	echo "problems $PROBLEMS"
	[ "$PROBLEMS" -eq 0 ] || return 3
	return 0
}

release() {
	tr -d ' \r\n' < "$SYS/release" 2>/dev/null || echo unknown
}

selftest_once() {
	head -n 1 "$SELFTEST_LOG" 2>/dev/null | grep -q " slot $(release)\$" && return 0
	selftest
}

# Copy a log, dropping anything that looks like a password or key.
scrub() {
	[ -f "$1" ] || return 0
	sed -E 's/(psk|password|passphrase|api_?key|key|token)([=:] *| +)[^ ]*/\1\2[removed]/Ig' \
		"$1" > "$2" 2>/dev/null
}

snapshot() {
	{
		echo "== slot"; cat "$SYS/release" 2>/dev/null
		echo "== BaseOS"; cat /etc/baseos-release 2>/dev/null
		echo "== kernel"; uname -a 2>/dev/null
		echo "== uptime"; cat /proc/uptime 2>/dev/null
		echo "== battery"
		for f in capacity status voltage_now current_now temp; do
			echo "$f=$(cat "$PSY/$f" 2>/dev/null)"
		done
		echo "== memory"; free 2>/dev/null || cat /proc/meminfo 2>/dev/null
		echo "== disk"; df -h 2>/dev/null
		echo "== cpu"
		for f in /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor \
			/sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq \
			/sys/class/thermal/thermal_zone0/temp; do
			echo "$f=$(cat "$f" 2>/dev/null)"
		done
		echo "== network"; ip addr 2>/dev/null
		echo "== modules"; lsmod 2>/dev/null
		echo "== processes"; ps 2>/dev/null
	} > "$1" 2>&1
}

bugreport() {
	why="${1:-asked}"
	selftest > /dev/null 2>&1
	name="bugreport-$(stamp)"
	[ "$why" = crash ] && name="crash-$(stamp)"
	dir="$REPORTS/$name"
	mkdir -p "$dir" 2>/dev/null || return 1
	echo "$why" > "$dir/reason.txt"
	for f in "$SD"/*.log "$SD"/*.log.[0-9]; do
		[ -f "$f" ] && scrub "$f" "$dir/$(basename "$f")"
	done
	if [ -d "$SD/crash" ]; then
		mkdir -p "$dir/crash"
		[ -f "$SD/crash/cores.log" ] && cp "$SD/crash/cores.log" "$dir/crash/"
		[ -f "$SD/crash/slot.maps" ] && cp "$SD/crash/slot.maps" "$dir/crash/"
		# The newest three crash logs; the core dumps are too big to carry.
		ls -t "$SD"/crash/slot-*.log 2>/dev/null | head -n 3 | while read -r f; do
			scrub "$f" "$dir/crash/$(basename "$f")"
		done
	fi
	dmesg 2>/dev/null | tail -n 300 > "$dir/dmesg.txt"
	snapshot "$dir/system.txt"
	# Only the newest few reports are kept, so the card does not fill up.
	ls -dt "$REPORTS"/*/ 2>/dev/null | tail -n +"$((KEEP + 1))" | while read -r old; do
		rm -rf "$old"
	done
	sync
	echo "saved $name"
	return 0
}

# A run that crashed says so in its log (see slot's crash.rs) or panicked.
crashcheck() {
	log="$SD/slot.log"
	[ -f "$log" ] || return 0
	grep -q -e 'slot: crash:' -e 'panicked at' "$log" 2>/dev/null || return 0
	# Run the report against the log of the crash, before it is rotated.
	bugreport crash > /dev/null 2>&1 || return 1
	mkdir -p "$SD/crash" 2>/dev/null
	echo "$(stamp)" > "$SD/crash/caught"
	return 0
}

case "${1:-}" in
selftest) selftest ;;
selftest-once) selftest_once ;;
bugreport) bugreport "${2:-asked}" ;;
crashcheck) crashcheck ;;
*)
	echo "usage: $0 selftest | selftest-once | bugreport [why] | crashcheck" >&2
	exit 1
	;;
esac
