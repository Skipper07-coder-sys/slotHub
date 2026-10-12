#!/bin/sh
# Bluetooth headphones for slot on BaseOS. BaseOS ships the RTL8821CS
# controller attach (setBluetooth.sh), BlueZ and BlueALSA; this script only
# drives them. Verbs:
#   on    attach the controller, start BlueZ and BlueALSA, reconnect paired headphones
#   off   stop all of it and block the radio
#   pair    search for headphones in pairing mode and pair, trust and connect the nearest
#   status  print "<address> <name>" of the headphones BlueALSA can play to
# Exit 0 on success, 3 when pair found no headphones, 1 otherwise.
set -u

SD="${SLOT_ROOT:-/mnt/sdcard}"
LOG="${AGS_BT_LOG:-$SD/slot-bluetooth.log}"
HCI="${AGS_BT_HCI:-/sys/class/bluetooth/hci0}"
VENDOR="${AGS_BT_VENDOR:-/mnt/vendor/ctrl/setBluetooth.sh}"
VENDOR_LOCK="${AGS_BT_VENDOR_LOCK:-/tmp/.init_bt}"
BTCTL="${AGS_BTCTL:-bluetoothctl}"
BLUEALSA="${AGS_BLUEALSA:-bluealsa}"
DBUS_SEND="${AGS_DBUS_SEND:-dbus-send}"
SYSTEMCTL="${AGS_SYSTEMCTL:-systemctl}"
SCAN_S="${AGS_BT_SCAN_S:-12}"
ALIAS="${AGS_BT_ALIAS:-slot}"

log() {
	echo "$(date '+%H:%M:%S') $*" >> "$LOG" 2>/dev/null
	return 0
}

# bluetoothctl can block forever on a sick daemon; bound every call.
btctl() {
	t="$1"
	shift
	if command -v timeout >/dev/null 2>&1; then
		timeout "$t" "$BTCTL" "$@" 2>/dev/null
	else
		"$BTCTL" "$@" 2>/dev/null
	fi
}

wait_for() {
	n=0
	while [ "$n" -lt "$2" ]; do
		eval "$1" && return 0
		sleep 1
		n=$((n + 1))
	done
	eval "$1"
}

attach() {
	[ -d "$HCI" ] && return 0
	[ -f "$VENDOR" ] || { log "no $VENDOR, this BaseOS has no Bluetooth"; return 1; }
	pidof rtk_hciattach >/dev/null 2>&1 && killall rtk_hciattach 2>/dev/null && sleep 1
	rm -f "$VENDOR_LOCK"
	log "attaching the controller"
	sh "$VENDOR" init >> "$LOG" 2>&1
	wait_for '[ -d "$HCI" ]' 8 || { log "hci0 never appeared"; return 1; }
	attached=1
}

start_bluealsa() {
	if pidof bluealsa >/dev/null 2>&1; then
		return 0
	fi
	# Keep BlueALSA's own volume mode: AirPods otherwise come up at volume zero.
	"$BLUEALSA" -p a2dp-source --initial-volume=100 < /dev/null >> "$LOG" 2>&1 &
	sleep 1
	pidof bluealsa >/dev/null 2>&1 && return 0
	"$BLUEALSA" -p a2dp-source < /dev/null >> "$LOG" 2>&1 &
	sleep 1
	pidof bluealsa >/dev/null 2>&1 || { log "bluealsa would not start"; return 1; }
}

up() {
	rfkill unblock bluetooth 2>/dev/null
	attached=0
	attach || return 1
	sh "$VENDOR" enable >> "$LOG" 2>&1
	if [ "$attached" = 1 ] && "$SYSTEMCTL" is-active bluetooth >/dev/null 2>&1; then
		"$SYSTEMCTL" restart bluetooth >> "$LOG" 2>&1
	else
		"$SYSTEMCTL" start bluetooth >> "$LOG" 2>&1
	fi
	wait_for 'btctl 5 show >/dev/null' 5 || { log "BlueZ never saw hci0"; return 1; }
	btctl 5 power on >/dev/null
	wait_for 'btctl 5 show | grep -q "Powered: yes"' 5 || { log "adapter will not power on"; return 1; }
	btctl 5 pairable on >/dev/null
	btctl 5 system-alias "$ALIAS" >/dev/null
	start_bluealsa || return 1
	log "bluetooth up"
}

paired() {
	list="$(btctl 5 paired-devices)"
	[ -n "$list" ] || list="$(btctl 5 devices Paired)"
	printf '%s\n' "$list" | sed -n 's/^Device \([0-9A-Fa-f:]\{17\}\).*/\1/p'
}

connected() {
	btctl 5 info "$1" | grep -q "Connected: yes"
}

audio() {
	btctl 5 info "$1" | grep -q -e "Icon: audio-" -e "Audio Sink"
}

name_of() {
	info="$(btctl 5 info "$1")"
	n="$(printf '%s\n' "$info" | sed -n 's/^[[:space:]]*Alias: //p' | head -n 1)"
	[ -n "$n" ] || n="$(printf '%s\n' "$info" | sed -n 's/^[[:space:]]*Name: //p' | head -n 1)"
	printf '%s\n' "${n% - Find My}"
}

# A link is not audio. BlueZ holds a media transport object
# (/org/bluez/hci0/dev_XX_XX_XX_XX_XX_XX/sepN/fdN on this BlueZ) exactly while an A2DP stream is
# configured, whatever BlueALSA version sits on top; this BlueALSA's
# bluealsa-aplay has no -L to ask it instead.
a2dp_ready() {
	if ! command -v "$DBUS_SEND" >/dev/null 2>&1; then
		# No way to see the stream: count the link, and let connect() ask anyway.
		connected "$1"
		return
	fi
	dev="dev_$(printf '%s' "$1" | tr ':' '_')"
	"$DBUS_SEND" --system --print-reply --dest=org.bluez / \
		org.freedesktop.DBus.ObjectManager.GetManagedObjects 2>/dev/null |
		grep -q -i -E "$dev(/sep[0-9]+)?/fd[0-9]"
}

# Pairing leaves AirPods linked with no A2DP: always ask for the profiles
# explicitly unless a stream already exists. Never disconnect here.
connect() {
	command -v "$DBUS_SEND" >/dev/null 2>&1 && a2dp_ready "$1" && return 0
	btctl 15 connect "$1" >> "$LOG"
	wait_for "a2dp_ready $1" 8 && return 0
	log "$1: no A2DP stream yet, asking again"
	btctl 15 connect "$1" >> "$LOG"
	wait_for "a2dp_ready $1" 8 && return 0
	log "$1: still no A2DP stream"
	return 1
}

reconnect() {
	for mac in $(paired); do
		audio "$mac" || continue
		if connect "$mac"; then
			log "reconnected $(name_of "$mac") ($mac)"
			return 0
		fi
	done
	return 0
}

status() {
	for mac in $(paired); do
		if audio "$mac" && a2dp_ready "$mac"; then
			echo "$mac $(name_of "$mac")"
			return 0
		fi
	done
	return 1
}

# Print "rank rssi mac" for each headphone the last scan saw, best first.
# AirPods can show up twice: as a classic device with a Class, which carries
# audio, and as a Bluetooth LE "<name> - Find My" entry. Prefer the classic one
# but never rule the other out, since BlueZ may merge both into one entry.
headphones() {
	for mac in $(btctl 5 devices | sed -n 's/^Device \([0-9A-Fa-f:]\{17\}\).*/\1/p'); do
		info="$(btctl 5 info "$mac")"
		printf '%s\n' "$info" | grep -q -e "Icon: audio-" -e "Audio Sink" || continue
		rank=0
		printf '%s\n' "$info" | grep -q "Class:" && rank=1
		rssi="$(printf '%s\n' "$info" | sed -n 's/.*RSSI: \(-\{0,1\}[0-9]*\).*/\1/p' | head -n 1)"
		echo "$rank ${rssi:--999} $mac"
	done | sort -k1,1nr -k2,2nr
}

pair() {
	up || return 1
	# A scan while A2DP is up tears the stream down a few seconds later.
	for mac in $(paired); do
		if audio "$mac" && a2dp_ready "$mac"; then
			log "$(name_of "$mac") ($mac) already playing, not searching"
			return 0
		fi
	done
	# Headphones paired before need a connect, not a new pairing: pairing a
	# bonded AirPods again hangs and ends in AuthenticationRejected.
	for mac in $(paired); do
		audio "$mac" || continue
		log "trying $(name_of "$mac") ($mac), paired before"
		if connect "$mac"; then
			log "connected $(name_of "$mac")"
			return 0
		fi
	done
	log "searching for headphones"
	{
		echo "scan on"
		sleep "$SCAN_S"
		echo "scan off"
		echo "quit"
	} | "$BTCTL" > /dev/null 2>&1
	found="$(headphones)"
	log "candidates: $(printf '%s\n' "$found" | tr '\n' ';')"
	mac="$(printf '%s\n' "$found" | head -n 1 | cut -d' ' -f3)"
	if [ -z "$mac" ]; then
		log "no headphones found"
		return 3
	fi
	name="$(name_of "$mac")"
	log "pairing ${name:-$mac} ($mac)"
	if ! btctl 5 info "$mac" | grep -q "Paired: yes"; then
		out="$(btctl 40 pair "$mac")"
		printf '%s\n' "$out" >> "$LOG"
		if ! wait_for "btctl 5 info $mac | grep -q 'Paired: yes'" 5; then
			# A stale bond makes BlueZ fail with AuthenticationRejected every time.
			# Forget it so the next press, with the case in pairing mode, starts clean.
			if printf '%s\n' "$out" | grep -q -e AuthenticationRejected -e AuthenticationFailed; then
				btctl 10 remove "$mac" >> "$LOG"
				log "pairing rejected; forgot $mac, put the headphones in pairing mode and try again"
			fi
			log "pairing failed"
			return 1
		fi
	fi
	btctl 5 trust "$mac" >/dev/null
	connect "$mac" || { log "paired but would not connect"; return 1; }
	log "connected ${name:-$mac}"
}

down() {
	if pidof bluetoothd >/dev/null 2>&1; then
		btctl 5 scan off >/dev/null
		btctl 5 power off >/dev/null
	fi
	killall bluetoothctl 2>/dev/null
	killall bluealsa 2>/dev/null
	"$SYSTEMCTL" stop bluetooth >> "$LOG" 2>&1
	hciconfig hci0 down 2>/dev/null
	killall rtk_hciattach 2>/dev/null
	rm -f "$VENDOR_LOCK"
	rfkill block bluetooth 2>/dev/null
	log "bluetooth off"
	return 0
}

case "${1:-}" in
	on) up && reconnect ;;
	off) down ;;
	pair) pair ;;
	status) status ;;
	*)
		echo "usage: $0 on|off|pair|status" >&2
		exit 1
		;;
esac
