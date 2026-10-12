#!/bin/sh
# Home Wi-Fi for slot on BaseOS. BaseOS loads the radio driver and brings wlan0
# up; joining a network is the frontend's job. This script joins the networks
# listed in wifi.conf at the card root and gets an address with udhcpc, whose
# BaseOS script also publishes <hostname>.local. Verbs:
#   up      join the first network in range, in the order wifi.conf lists them
#   down    leave it and stop the supplicant and DHCP client
#   pause   leave it for link play, remembering to come back
#   resume  come back after link play, if pause left it
#   status  print the IPv4 address, if there is one
#   scan    print the networks in range, strongest first, one per line as
#           <signal dBm><TAB><1 if it needs a password, else 0><TAB><name>
# Exit 0 on success, 3 when wifi.conf names no usable network, 4 when none of
# them could be joined, 5 when joined but no address came, 1 otherwise.
set -u

SD="${SLOT_ROOT:-/mnt/sdcard}"
CONF="${AGS_WIFI_CONF:-$SD/wifi.conf}"
LOG="${AGS_WIFI_LOG:-$SD/slot-wifi.log}"
NET="${AGS_NET_SYS:-/sys/class/net}"
RUN="${AGS_RUN:-/run}"
CTRL_DIR="${AGS_CTRL_DIR:-/var/run/wpa_supplicant}"
WPA_SUPPLICANT="${AGS_WPA_SUPPLICANT:-wpa_supplicant}"
WPA_CLI="${AGS_WPA_CLI:-wpa_cli}"
IP="${AGS_IP:-ip}"
IW="${AGS_IW:-iw}"
UDHCPC="${AGS_UDHCPC:-udhcpc}"
JOIN_S="${AGS_WIFI_JOIN_S:-20}"
ADDR_S="${AGS_WIFI_ADDR_S:-15}"
SCAN_S="${AGS_WIFI_SCAN_S:-3}"
IFACE_S="${AGS_WIFI_IFACE_S:-25}"
SCAN_TRIES="${AGS_WIFI_SCAN_TRIES:-3}"
SCAN_WAIT_S="${AGS_WIFI_SCAN_WAIT_S:-12}"

SUP_CONF="$RUN/slot-wifi.conf"
DHCP_PID="$RUN/slot-wifi-udhcpc.pid"
ON="$RUN/slot-wifi.on"
PAUSED="$RUN/slot-wifi.paused"
# The joined network's name, for the settings menu's Wi-Fi row.
SSID="$RUN/slot-wifi.ssid"

log() {
	echo "$(date '+%H:%M:%S') $*" >> "$LOG" 2>/dev/null
	return 0
}

# One verb at a time: link play's resume runs in the background and could
# otherwise bring the home network back up in the middle of a pause, or race
# slot's own up into two supplicants. A lock whose holder is gone is taken over.
LOCK="$RUN/slot-wifi.lock"
LOCK_S="${AGS_WIFI_LOCK_S:-90}"

lock() {
	waited=0
	while ! mkdir "$LOCK" 2>/dev/null; do
		holder="$(cat "$LOCK/pid" 2>/dev/null)"
		if [ -n "$holder" ] && ! kill -0 "$holder" 2>/dev/null; then
			/bin/rm -rf "$LOCK"
			continue
		fi
		if [ "$waited" -ge "$LOCK_S" ]; then
			log "lock: still held by ${holder:-?} after ${LOCK_S}s, going ahead"
			break
		fi
		sleep 1
		waited=$((waited + 1))
	done
	echo $$ > "$LOCK/pid" 2>/dev/null
	trap '/bin/rm -rf "$LOCK"' EXIT
}

hex() {
	printf '%s' "$1" | od -An -tx1 | tr -d ' \n'
}

# wifi.conf: one ssid= line per network, each followed by its psk= (leave the
# psk out for an open network). Lines starting with # are comments.
write_conf() {
	n=0
	prio=100
	ssid=
	psk=
	have=0
	{
		echo "ctrl_interface=$CTRL_DIR"
		echo 'update_config=0'
	} > "$SUP_CONF.tmp"
	/bin/chmod 600 "$SUP_CONF.tmp"
	while IFS= read -r line || [ -n "$line" ]; do
		line=$(printf '%s' "$line" | tr -d '\r')
		case "$line" in
		'#'* | '') continue ;;
		ssid=*)
			[ "$have" = 1 ] && add_network && n=$((n + 1))
			ssid="${line#ssid=}"
			psk=
			have=1
			;;
		psk=* | password=*)
			psk="${line#*=}"
			;;
		esac
	done < "$CONF"
	[ "$have" = 1 ] && add_network && n=$((n + 1))
	mv -f "$SUP_CONF.tmp" "$SUP_CONF"
	[ "$n" -gt 0 ]
}

add_network() {
	[ -n "$ssid" ] || return 1
	if [ -n "$psk" ]; then
		len=${#psk}
		if [ "$len" -eq 64 ] && ! printf '%s' "$psk" | grep -q '[^0-9a-fA-F]'; then
			key="psk=$psk"
		elif [ "$len" -ge 8 ] && [ "$len" -le 63 ]; then
			key="psk=\"$psk\""
		else
			log "skipping $ssid: a password is 8 to 63 characters"
			return 1
		fi
	fi
	{
		echo 'network={'
		echo "	ssid=$(hex "$ssid")"
		echo '	scan_ssid=1'
		if [ -n "$psk" ]; then
			echo "	$key"
		else
			echo '	key_mgmt=NONE'
		fi
		echo "	priority=$prio"
		echo '}'
	} >> "$SUP_CONF.tmp"
	prio=$((prio - 1))
	return 0
}

supplicant_stop() {
	if $WPA_CLI -p "$CTRL_DIR" -i wlan0 terminate >/dev/null 2>&1; then
		s=0
		while [ -e "$CTRL_DIR/wlan0" ] && [ "$s" -lt 8 ]; do
			sleep 0.25 2>/dev/null || sleep 1
			s=$((s + 1))
		done
	elif [ -e "$CTRL_DIR/wlan0" ]; then
		/bin/rm -f "$CTRL_DIR/wlan0"
	fi
	return 0
}

dhcp_stop() {
	pid="$(cat "$DHCP_PID" 2>/dev/null)"
	[ -n "$pid" ] && kill "$pid" 2>/dev/null
	/bin/rm -f "$DHCP_PID"
	return 0
}

address() {
	$IP -4 addr show dev wlan0 2>/dev/null | sed -n 's/^ *inet \([0-9.]*\).*/\1/p' | head -n 1
}

wait_state() {
	end=$(($(date +%s) + JOIN_S))
	was=
	while :; do
		state=$($WPA_CLI -p "$CTRL_DIR" -i wlan0 status 2>/dev/null | sed -n 's/^wpa_state=//p')
		[ "$state" != "$was" ] && log "wlan0 ${state:-no answer}" && was=$state
		[ "$state" = COMPLETED ] && return 0
		[ "$(date +%s)" -ge "$end" ] && return 1
		sleep 0.25 2>/dev/null || sleep 1
	done
}

wait_address() {
	end=$(($(date +%s) + ADDR_S))
	while [ -z "$(address)" ]; do
		[ "$(date +%s)" -ge "$end" ] && return 1
		sleep 0.25 2>/dev/null || sleep 1
	done
	return 0
}

# BaseOS brings the radio up in the background after boot, and reloads the
# driver once on units with a slow radio, so wlan0 can take a while to appear.
wait_wlan0() {
	end=$(($(date +%s) + IFACE_S))
	while [ ! -d "$NET/wlan0" ]; do
		[ "$(date +%s)" -ge "$end" ] && return 1
		sleep 0.25 2>/dev/null || sleep 1
	done
	return 0
}

leave() {
	/bin/rm -f "$SSID"
	dhcp_stop
	supplicant_stop
	$IP addr flush dev wlan0 2>/dev/null || true
}

wifi_up() {
	/bin/rm -f "$PAUSED"
	: > "$ON"
	if [ ! -f "$CONF" ]; then
		log "no $CONF"
		return 3
	fi
	write_conf || { log "no usable network in $CONF"; return 3; }
	wait_wlan0 || { log "no wlan0 in ${IFACE_S}s"; return 1; }
	rfkill unblock all 2>/dev/null || true
	$IP link set wlan0 up 2>/dev/null || true
	command -v "$IW" >/dev/null 2>&1 && $IW dev wlan0 set power_save off 2>/dev/null
	leave
	/bin/mkdir -p "$CTRL_DIR"
	if ! $WPA_SUPPLICANT -B -i wlan0 -c "$SUP_CONF" -Dnl80211 >/dev/null 2>&1; then
		log "wpa_supplicant would not start"
		return 1
	fi
	if ! wait_state; then
		log "no network joined in ${JOIN_S}s"
		# A join that lands later would sit there with no address; start
		# clean on the next try instead.
		leave
		return 4
	fi
	$UDHCPC -i wlan0 -b -t 5 -T 3 -p "$DHCP_PID" -x "hostname:$(hostname 2>/dev/null || echo slot)" \
		>/dev/null 2>&1 || true
	if ! wait_address; then
		log "joined but no address in ${ADDR_S}s"
		return 5
	fi
	$WPA_CLI -p "$CTRL_DIR" -i wlan0 status 2>/dev/null | sed -n 's/^ssid=//p' > "$SSID" 2>/dev/null
	log "online at $(address) on $(cat "$SSID" 2>/dev/null)"
	return 0
}

wifi_down() {
	/bin/rm -f "$ON" "$PAUSED"
	leave
	log "off"
	return 0
}

wifi_pause() {
	[ -e "$ON" ] || return 0
	/bin/rm -f "$ON"
	: > "$PAUSED"
	leave
	log "paused for link play"
	return 0
}

wifi_resume() {
	[ -e "$PAUSED" ] || return 0
	log "resuming after link play"
	wifi_up
}

# iw's scan output: one "BSS" block per network with its signal, flags and name.
iw_scan_lines() {
	awk '
	function flush() { if (ssid != "" && sig != "") print sig "\t" lock "\t" ssid }
	/^BSS / { flush(); sig = ""; ssid = ""; lock = 0 }
	/^[ \t]*signal:/ { sig = $2; sub(/\..*/, "", sig) }
	/^[ \t]*SSID:/ { ssid = substr($0, index($0, "SSID:") + 6) }
	/^[ \t]*capability:.*Privacy/ { lock = 1 }
	END { flush() }'
}

# Scan the way muOS does on this chip: iw, asked up to three times two seconds
# apart, because the radio answers "busy" or hears nothing right after coming
# up. Without iw, borrow the running supplicant or start a bare one and stop
# it again so a scan with Wi-Fi off leaves it off.
wifi_scan() {
	wait_wlan0 || { log "scan: no wlan0 in ${IFACE_S}s"; return 1; }
	rfkill unblock all 2>/dev/null || true
	$IP link set wlan0 up 2>/dev/null || true
	log "scan: iw=$(command -v "$IW" 2>/dev/null || echo none) wpa_cli=$(command -v "$WPA_CLI" 2>/dev/null || echo none) wpa_supplicant=$(command -v "$WPA_SUPPLICANT" 2>/dev/null || echo none) wlan0=$(cat "$NET/wlan0/operstate" 2>/dev/null || echo ?)"
	if command -v "$IW" >/dev/null 2>&1; then
		$IW dev wlan0 set power_save off 2>/dev/null || true
		sleep 1
		tries=0
		results=
		while :; do
			tries=$((tries + 1))
			raw="$($IW dev wlan0 scan 2>&1)"
			results="$(printf '%s\n' "$raw" | iw_scan_lines)"
			[ -n "$results" ] || [ "$tries" -ge "$SCAN_TRIES" ] && break
			log "scan: pass $tries heard nothing: $(printf '%s\n' "$raw" | head -n 2 | tr '\n' ' ')"
			sleep "$SCAN_S"
		done
		log "scan: $(printf '%s\n' "$results" | grep -c .) results on pass $tries (iw)"
		printf '%s\n' "$results" | grep . | sort -t "$(printf '\t')" -k1,1nr | awk -F '\t' '!seen[$3]++'
		return 0
	fi
	bare=0
	if ! $WPA_CLI -p "$CTRL_DIR" -i wlan0 ping 2>/dev/null | grep -q PONG; then
		/bin/mkdir -p "$CTRL_DIR"
		echo "ctrl_interface=$CTRL_DIR" > "$RUN/slot-wifi-scan.conf"
		$WPA_SUPPLICANT -B -i wlan0 -c "$RUN/slot-wifi-scan.conf" -Dnl80211 >/dev/null 2>&1 \
			|| { log "scan: wpa_supplicant would not start"; return 1; }
		bare=1
		w=0
		while ! $WPA_CLI -p "$CTRL_DIR" -i wlan0 ping 2>/dev/null | grep -q PONG && [ "$w" -lt 12 ]; do
			sleep 0.25 2>/dev/null || sleep 1
			w=$((w + 1))
		done
	fi
	if ! $WPA_CLI -p "$CTRL_DIR" -i wlan0 ping 2>/dev/null | grep -q PONG; then
		log "scan: the supplicant is not answering"
	fi
	# A scan on this radio takes four to five seconds, so ask once and read the
	# list every second until it fills, asking again halfway if it stays empty.
	asked="$($WPA_CLI -p "$CTRL_DIR" -i wlan0 scan 2>&1 | tr '\n' ' ')"
	tries=0
	results=
	while :; do
		sleep 1
		tries=$((tries + 1))
		raw="$($WPA_CLI -p "$CTRL_DIR" -i wlan0 scan_results 2>&1)"
		results="$(printf '%s\n' "$raw" | tail -n +2)"
		[ -n "$results" ] || [ "$tries" -ge "$SCAN_WAIT_S" ] && break
		if [ "$tries" -eq $((SCAN_WAIT_S / 2)) ]; then
			log "scan: nothing after ${tries}s: scan said '$asked', results '$(printf '%s\n' "$raw" | head -n 2 | tr '\n' ' ')'"
			asked="$($WPA_CLI -p "$CTRL_DIR" -i wlan0 scan 2>&1 | tr '\n' ' ')"
		fi
	done
	log "scan: $(printf '%s\n' "$results" | grep -c .) results after ${tries}s"
	printf '%s\n' "$results" \
		| awk -F '\t' 'NF >= 5 && $5 != "" && $3 ~ /^-?[0-9]+$/ {
			locked = ($4 ~ /WPA|WEP|RSN|SAE/) ? 1 : 0
			print $3 "\t" locked "\t" $5
		}' \
		| sort -t "$(printf '\t')" -k1,1nr \
		| awk -F '\t' '!seen[$3]++'
	if [ "$bare" = 1 ]; then
		supplicant_stop
		/bin/rm -f "$RUN/slot-wifi-scan.conf"
	fi
	return 0
}

case "${1:-}" in
up | down | pause | resume | scan) lock ;;
esac

case "${1:-}" in
up) wifi_up ;;
scan) wifi_scan ;;
down) wifi_down ;;
pause) wifi_pause ;;
resume) wifi_resume ;;
status)
	a="$(address)"
	[ -n "$a" ] && echo "$a"
	[ -n "$a" ]
	;;
*)
	echo "usage: wifi.sh [up|down|pause|resume|status|scan]" >&2
	exit 2
	;;
esac
