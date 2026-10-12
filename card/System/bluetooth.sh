#!/bin/sh
# Bluetooth headphones for slot on BaseOS. BaseOS ships the RTL8821CS
# controller attach (setBluetooth.sh), BlueZ and BlueALSA; this script only
# drives them. Verbs:
#   on        attach the controller, start BlueZ and BlueALSA, reconnect the remembered headphones
#   off       power the adapter down and stop BlueALSA; the controller stays attached
#   scan      search for about 12 s and print "rssi<TAB>paired<TAB>address<TAB>name" per headphones
#   connect A pair (if needed), trust and connect the headphones at address A, and remember them
#   status    print "<address> <name>" of the headphones BlueZ streams to
#   debug     write everything BlueZ and BlueALSA know to the log
# Exit 0 on success, 4 when pairing was refused (put the headphones in pairing
# mode), 1 otherwise. Everything else goes to slot-bluetooth.log on the card.
set -u

SD="${SLOT_ROOT:-/mnt/sdcard}"
LOG="${AGS_BT_LOG:-$SD/slot-bluetooth.log}"
CONF="${AGS_BT_CONF:-$SD/bluetooth.conf}"
HCI="${AGS_BT_HCI:-/sys/class/bluetooth/hci0}"
VENDOR="${AGS_BT_VENDOR:-/mnt/vendor/ctrl/setBluetooth.sh}"
VENDOR_LOCK="${AGS_BT_VENDOR_LOCK:-/tmp/.init_bt}"
BTCTL="${AGS_BTCTL:-bluetoothctl}"
BLUEALSA="${AGS_BLUEALSA:-bluealsa}"
DBUS_SEND="${AGS_DBUS_SEND:-dbus-send}"
SYSTEMCTL="${AGS_SYSTEMCTL:-systemctl}"
SCAN_S="${AGS_BT_SCAN_S:-12}"
# How long the search keeps running while a pairing is attempted.
PAIR_SCAN_S="${AGS_BT_PAIR_SCAN_S:-60}"
ALIAS="${AGS_BT_ALIAS:-slot}"
STATUS_SEEN="${AGS_BT_STATUS_SEEN:-/tmp/slot-bt-status}"
# A2DP volume (0-127) given to headphones that come up silent.
VOLUME="${AGS_BT_VOLUME:-90}"

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

have_dbus() {
	command -v "$DBUS_SEND" >/dev/null 2>&1
}

attach() {
	[ -d "$HCI" ] && return 0
	[ -f "$VENDOR" ] || { log "no $VENDOR, this BaseOS has no Bluetooth"; return 1; }
	pidof rtk_hciattach >/dev/null 2>&1 && killall rtk_hciattach 2>/dev/null && sleep 1
	# setBluetooth.sh skips the attach while its lock file exists.
	rm -f "$VENDOR_LOCK"
	log "attaching the controller"
	sh "$VENDOR" init >> "$LOG" 2>&1
	wait_for '[ -d "$HCI" ]' 8 || { log "hci0 never appeared"; return 1; }
	attached=1
}

# Closing the PCM with keep-alive 0 made BlueALSA tear the A2DP stream down
# within milliseconds (seen on the card), so ask it to hold the stream. The
# option was renamed across BlueALSA versions; fall back until one starts.
# --initial-volume keeps AirPods from coming up at volume zero.
start_bluealsa() {
	pidof bluealsa >/dev/null 2>&1 && return 0
	for opts in \
		"-p a2dp-source --initial-volume=100 --keep-alive=3600" \
		"-p a2dp-source --initial-volume=100 --a2dp-keep-alive=3600" \
		"-p a2dp-source --initial-volume=100" \
		"-p a2dp-source"; do
		# shellcheck disable=SC2086
		"$BLUEALSA" $opts < /dev/null >> "$LOG" 2>&1 &
		sleep 1
		if pidof bluealsa >/dev/null 2>&1; then
			log "bluealsa $opts"
			return 0
		fi
	done
	log "bluealsa would not start"
	return 1
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
	log "bluetooth up, controller $(btctl 5 show | sed -n 's/^Controller \([0-9A-Fa-f:]\{17\}\).*/\1/p' | head -n 1)"
}

addresses() {
	sed -n 's/^Device \([0-9A-Fa-f:]\{17\}\).*/\1/p'
}

known() {
	btctl 5 devices | addresses
}

# Every device BlueZ marks paired, judged from each device's own info: the
# list commands differ between bluetoothctl versions and one of them came
# back empty on the card even right after a pairing.
paired() {
	for mac in $(known); do
		btctl 5 info "$mac" | grep -q "Paired: yes" && echo "$mac"
	done
}

connected() {
	btctl 5 info "$1" | grep -q "Connected: yes"
}

# Headphones and speakers carry the A2DP Audio Sink service (UUID 110b), but
# BlueZ lists a device's services only once it has connected to it; freshly
# found headphones show a class and an icon and nothing else, and on the card
# the AirPods vanished from the list the moment their stale bond was removed.
# So the service decides when it is listed (a TV advertising only Audio Source
# matched an audio icon, got paired, and refused to connect), and until then
# the device class does, as the muOS Bluetooth app for the RG35XX SP has it:
# major class Audio/Video with a headset, headphones or speaker minor class.
sink() {
	if printf '%s\n' "$1" | grep -q -i "UUID:"; then
		printf '%s\n' "$1" | grep -q -i -e "0000110b-" -e "Audio Sink"
		return
	fi
	class="$(printf '%s\n' "$1" | sed -n 's/^[[:space:]]*Class: 0x\([0-9A-Fa-f]\{1,\}\).*/\1/p' | head -n 1)"
	[ -n "$class" ] || return 1
	c=$((0x$class))
	[ $(((c >> 8) & 0x1f)) -eq 4 ] || return 1
	case $(((c >> 2) & 0x3f)) in
	1 | 2 | 5 | 6 | 7 | 8 | 10) return 0 ;;
	esac
	return 1
}

name_of() {
	info="$(btctl 5 info "$1")"
	n="$(printf '%s\n' "$info" | sed -n 's/^[[:space:]]*Alias: //p' | head -n 1)"
	[ -n "$n" ] || n="$(printf '%s\n' "$info" | sed -n 's/^[[:space:]]*Name: //p' | head -n 1)"
	printf '%s\n' "${n% - Find My}"
}

# bluetoothctl prints "RSSI: -60" or "RSSI: 0xffffffc4 (-60)"; -999 when unseen.
rssi_of() {
	r="$(printf '%s\n' "$1" | sed -n 's/.*RSSI:.*(\(-\{0,1\}[0-9]\{1,\}\)).*/\1/p' | head -n 1)"
	[ -n "$r" ] || r="$(printf '%s\n' "$1" | sed -n 's/.*RSSI: \(-\{0,1\}[0-9]\{1,\}\)[[:space:]]*$/\1/p' | head -n 1)"
	printf '%s\n' "${r:--999}"
}

# A link is not audio. BlueZ holds a media transport object
# (/org/bluez/hci0/dev_XX_XX_XX_XX_XX_XX/sepN/fdN on this BlueZ) exactly while
# an A2DP stream is configured, whatever BlueALSA version sits on top. Prints
# the address of every device with one.
managed() {
	"$DBUS_SEND" --system --print-reply --dest=org.bluez / \
		org.freedesktop.DBus.ObjectManager.GetManagedObjects 2>/dev/null
}

transports() {
	managed |
		sed -n 's/.*\/dev_\([0-9A-Fa-f_]\{17\}\)\(\/sep[0-9]*\)\{0,1\}\/fd[0-9].*/\1/p' |
		tr '_a-f' ':A-F' | sort -u
}

# Object path of each media transport of one device.
transport_paths() {
	dev="dev_$(printf '%s' "$1" | tr ':' '_')"
	managed |
		sed -n 's/.*object path "\(\/org\/bluez\/hci[0-9]*\/'"$dev"'\(\/sep[0-9]*\)\{0,1\}\/fd[0-9]*\)".*/\1/p' |
		sort -u
}

transport_volume() {
	"$DBUS_SEND" --system --print-reply --dest=org.bluez "$1" \
		org.freedesktop.DBus.Properties.Get \
		string:org.bluez.MediaTransport1 string:Volume 2>/dev/null |
		sed -n 's/.*uint16 \([0-9]*\).*/\1/p' | head -n 1
}

# AirPods told the card their volume was 0 as the stream came up, which plays
# in silence. Give silent headphones a volume; leave any other alone.
fix_volume() {
	have_dbus || return 0
	for path in $(transport_paths "$1"); do
		v="$(transport_volume "$path")"
		log "$1: headphone volume ${v:-unknown} (0-127)"
		if [ "${v:-1}" = 0 ]; then
			"$DBUS_SEND" --system --print-reply --dest=org.bluez "$path" \
				org.freedesktop.DBus.Properties.Set \
				string:org.bluez.MediaTransport1 string:Volume "variant:uint16:$VOLUME" >/dev/null 2>&1
			log "$1: set the headphone volume to $VOLUME"
		fi
	done
}

a2dp_ready() {
	if ! have_dbus; then
		# No way to see the stream: count the link, and let connect() ask anyway.
		connected "$1"
		return
	fi
	transports | grep -q -i -x "$1"
}

# Pairing leaves AirPods linked with no A2DP: always ask for the profiles
# explicitly unless a stream already exists. Never disconnect here.
connect() {
	if ! { have_dbus && a2dp_ready "$1"; }; then
		btctl 15 connect "$1" >> "$LOG"
		if ! wait_for "a2dp_ready $1" 8; then
			log "$1: no A2DP stream yet, asking again"
			btctl 15 connect "$1" >> "$LOG"
			wait_for "a2dp_ready $1" 8 || { log "$1: still no A2DP stream"; return 1; }
		fi
	fi
	fix_volume "$1"
	return 0
}

# The headphones the user chose, kept on the card so reconnecting never
# depends on what BlueZ remembers.
remembered() {
	sed -n 's/^mac=//p' "$CONF" 2>/dev/null | head -n 1
}

remember() {
	printf 'mac=%s\nname=%s\n' "$1" "$2" > "$CONF.tmp" 2>/dev/null && mv -f "$CONF.tmp" "$CONF"
}

reconnect() {
	for mac in $(remembered) $(paired); do
		info="$(btctl 5 info "$mac")"
		[ -n "$info" ] && sink "$info" || continue
		log "reconnecting $(name_of "$mac") ($mac)"
		if connect "$mac"; then
			log "reconnected $(name_of "$mac")"
			return 0
		fi
	done
	return 0
}

# The transport itself says where audio can go, so status does not depend on
# BlueZ still listing the device as paired. Changes are logged, not every poll.
status() {
	if have_dbus; then
		macs="$(transports)"
	else
		macs=""
		for mac in $(paired); do
			connected "$mac" && macs="$macs $mac"
		done
	fi
	line=""
	for mac in $macs; do
		line="$mac $(name_of "$mac")"
		break
	done
	last="$(cat "$STATUS_SEEN" 2>/dev/null)"
	if [ "$line" != "$last" ]; then
		log "status: ${line:-no headphones playing}"
		printf '%s' "$line" > "$STATUS_SEEN" 2>/dev/null
	fi
	[ -n "$line" ] || return 1
	echo "$line"
}

# Every sink BlueZ knows, nearest first, as "rssi<TAB>paired<TAB>mac<TAB>name".
sinks() {
	for mac in $(known); do
		info="$(btctl 5 info "$mac")"
		sink "$info" || continue
		p=0
		printf '%s\n' "$info" | grep -q "Paired: yes" && p=1
		printf '%s\t%s\t%s\t%s\n' "$(rssi_of "$info")" "$p" "$mac" "$(name_of "$mac")"
	done | sort -t "$(printf '\t')" -k1,1nr
}

# BlueZ reports a device's RSSI only while a search is running (every device
# came back -999 on the card once it had stopped), so the list is read before
# the search ends.
scan() {
	up || return 1
	log "searching for headphones"
	"$BTCTL" --timeout "$((SCAN_S + 10))" scan on > /dev/null 2>&1 &
	scanner=$!
	sleep "$SCAN_S"
	found="$(sinks)"
	stop_search
	log "found: $(printf '%s\n' "$found" | tr '\t' ' ' | tr '\n' ';')"
	[ -n "$found" ] && printf '%s\n' "$found"
	return 0
}

# BlueZ drops an unpaired device from its list about 30 s after a search ends,
# so by the time a pair is picked from the list it can be gone ("not
# available"), and pairing then fails at once. The search runs again in the
# background until the pairing is over, which also keeps the headphones' class
# and name current; the muOS app does the same.
pair_with() {
	btctl 5 info "$1" | grep -q "Paired: yes" && return 0
	"$BTCTL" --timeout "$PAIR_SCAN_S" scan on > /dev/null 2>&1 &
	scanner=$!
	if ! wait_for "btctl 5 info $1 | grep -q -e 'Name:' -e 'Alias:'" 20; then
		stop_search
		log "$1 is not in range; put the headphones in pairing mode and try again"
		return 1
	fi
	out="$(btctl 40 pair "$1")"
	stop_search
	printf '%s\n' "$out" >> "$LOG"
	wait_for "btctl 5 info $1 | grep -q 'Paired: yes'" 5 && return 0
	# A stale bond makes BlueZ fail with AuthenticationRejected every time.
	# Forget it so the next try, with the case in pairing mode, starts clean.
	if printf '%s\n' "$out" | grep -q -e AuthenticationRejected -e AuthenticationFailed; then
		btctl 10 remove "$1" >> "$LOG"
		log "pairing refused; forgot $1, put the headphones in pairing mode and try again"
		return 4
	fi
	# No answer at all within the pairing window: the headphones were not
	# listening for a new pairing (on the card the AirPods timed out twice
	# outside pairing mode, then paired at once inside it).
	if [ -z "$(printf '%s\n' "$out" | grep -i -e error -e failed)" ]; then
		log "no answer from $1; put the headphones in pairing mode and try again"
		return 4
	fi
	log "pairing failed"
	return 1
}

stop_search() {
	kill "$scanner" 2>/dev/null
	btctl 5 scan off > /dev/null 2>&1
}

# Trusted before paired: headphones connect back on their own right after
# pairing, and BlueZ lets a trusted device in without asking the agent.
connect_to() {
	mac="$1"
	up || return 1
	name="$(name_of "$mac")"
	log "connecting ${name:-$mac} ($mac)"
	btctl 5 trust "$mac" >/dev/null
	pair_with "$mac" || return $?
	name="$(name_of "$mac")"
	if ! connect "$mac"; then
		log "${name:-$mac} paired but would not connect"
		debug
		return 1
	fi
	remember "$mac" "$name"
	log "connected ${name:-$mac}"
}

down() {
	# The controller stays attached: re-running the vendor attach after an
	# off hung the card once, and a powered-down adapter costs nothing.
	if pidof bluetoothd >/dev/null 2>&1; then
		btctl 5 scan off >/dev/null
		btctl 5 power off >/dev/null
	fi
	killall bluetoothctl 2>/dev/null
	killall bluealsa 2>/dev/null
	rm -f "$STATUS_SEEN"
	log "bluetooth off"
	return 0
}

debug() {
	{
		echo "--- debug $(date '+%H:%M:%S')"
		echo "remembered: $(remembered)"
		echo "bluealsa pid: $(pidof bluealsa 2>/dev/null)"
		echo "# show"; btctl 5 show
		echo "# devices"; btctl 5 devices
		echo "# devices Paired"; btctl 5 devices Paired
		for mac in $(known); do
			echo "# info $mac"
			btctl 5 info "$mac"
		done
		echo "# transports"; have_dbus && transports
		echo "--- end debug"
	} >> "$LOG" 2>&1
	return 0
}

case "${1:-}" in
	on) up && reconnect && debug ;;
	off) down ;;
	scan) scan ;;
	connect)
		[ -n "${2:-}" ] || { echo "usage: $0 connect <address>" >&2; exit 1; }
		connect_to "$2"
		;;
	status) status ;;
	debug) debug ;;
	*)
		echo "usage: $0 on|off|scan|connect <address>|status|debug" >&2
		exit 1
		;;
esac
