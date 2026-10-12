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
UDHCPC="${AGS_UDHCPC:-udhcpc}"
JOIN_S="${AGS_WIFI_JOIN_S:-20}"
ADDR_S="${AGS_WIFI_ADDR_S:-15}"

SUP_CONF="$RUN/slot-wifi.conf"
DHCP_PID="$RUN/slot-wifi-udhcpc.pid"
ON="$RUN/slot-wifi.on"
PAUSED="$RUN/slot-wifi.paused"

log() {
	echo "$(date '+%H:%M:%S') $*" >> "$LOG" 2>/dev/null
	return 0
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

leave() {
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
	w=0
	while [ ! -d "$NET/wlan0" ] && [ "$w" -lt 40 ]; do
		sleep 0.25 2>/dev/null || sleep 1
		w=$((w + 1))
	done
	[ -d "$NET/wlan0" ] || { log "no wlan0"; return 1; }
	rfkill unblock wifi 2>/dev/null || true
	$IP link set wlan0 up 2>/dev/null || true
	leave
	/bin/mkdir -p "$CTRL_DIR"
	if ! $WPA_SUPPLICANT -B -i wlan0 -c "$SUP_CONF" -Dnl80211 >/dev/null 2>&1; then
		log "wpa_supplicant would not start"
		return 1
	fi
	if ! wait_state; then
		log "no network joined in ${JOIN_S}s"
		return 4
	fi
	$UDHCPC -i wlan0 -b -t 5 -T 3 -p "$DHCP_PID" -x "hostname:$(hostname 2>/dev/null || echo slot)" \
		>/dev/null 2>&1 || true
	if ! wait_address; then
		log "joined but no address in ${ADDR_S}s"
		return 5
	fi
	log "online at $(address)"
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

case "${1:-}" in
up) wifi_up ;;
down) wifi_down ;;
pause) wifi_pause ;;
resume) wifi_resume ;;
status)
	a="$(address)"
	[ -n "$a" ] && echo "$a"
	[ -n "$a" ]
	;;
*)
	echo "usage: wifi.sh [up|down|pause|resume|status]" >&2
	exit 2
	;;
esac
