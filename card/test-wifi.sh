#!/bin/sh
set -eu

HERE="$(cd "$(dirname "$0")" && pwd)"
SCRIPT="$HERE/System/wifi.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT HUP INT TERM
FAILS=0

fail() {
	echo "FAIL: $*"
	FAILS=$((FAILS + 1))
}

make_fake() {
	name="$1"
	out="$2"
	shift 2
	{
		echo '#!/bin/sh'
		echo "printf '%s\\n' \"\$*\" >> \"$out\""
		for line in "$@"; do echo "$line"; done
		echo 'exit 0'
	} > "$TMP/bin/$name"
	chmod 755 "$TMP/bin/$name"
}

# ip answers with an address once udhcpc has run, as the real lease would.
reset() {
	rm -rf "$TMP/net" "$TMP/run" "$TMP/ctrl" "$TMP"/*.log "$TMP/bin" "$TMP/leased" "$TMP/wifi.conf"
	mkdir -p "$TMP/net/wlan0" "$TMP/run" "$TMP/ctrl" "$TMP/bin"
	make_fake wpa_cli "$TMP/cli.log" 'echo "wpa_state=${FAKE_STATE:-COMPLETED}"'
	make_fake wpa_supplicant "$TMP/sup.log"
	make_fake ip "$TMP/ip.log" \
		"[ -e \"$TMP/leased\" ] && case \"\$*\" in *'addr show'*) echo '    inet 192.168.1.42/24 brd 192.168.1.255 scope global wlan0' ;; esac"
	make_fake udhcpc "$TMP/dhcp.log" "${FAKE_LEASE:-: > \"$TMP/leased\"}"
}

run() {
	PATH="$TMP/bin:$PATH" SLOT_ROOT="$TMP" \
		AGS_NET_SYS="$TMP/net" AGS_RUN="$TMP/run" AGS_CTRL_DIR="$TMP/ctrl" \
		AGS_WPA_SUPPLICANT="$TMP/bin/wpa_supplicant" AGS_WPA_CLI="$TMP/bin/wpa_cli" \
		AGS_IP="$TMP/bin/ip" AGS_UDHCPC="$TMP/bin/udhcpc" \
		AGS_WIFI_JOIN_S=1 AGS_WIFI_ADDR_S=1 \
		sh "$SCRIPT" "$@"
}

reset
run up && rc=0 || rc=$?
[ "$rc" = 3 ] || fail "up with no wifi.conf exited $rc, want 3"
[ -e "$TMP/sup.log" ] && fail "up with no wifi.conf started the supplicant"

reset
printf '# home\r\nssid=My "Home"\r\npsk=correct horse\r\n\r\nssid=Cafe\n' > "$TMP/wifi.conf"
run up && rc=0 || rc=$?
[ "$rc" = 0 ] || fail "up exited $rc"
conf="$TMP/run/slot-wifi.conf"
grep -q "ssid=4d792022486f6d6522$" "$conf" || fail "ssid not written as hex: $(grep ssid= "$conf")"
grep -q 'psk="correct horse"$' "$conf" || fail "psk lost or kept its carriage return"
grep -q 'key_mgmt=NONE' "$conf" || fail "open network not written open"
[ "$(grep -c 'network={' "$conf")" = 2 ] || fail "want two networks"
p1=$(sed -n 's/.*priority=//p' "$conf" | head -n 1)
p2=$(sed -n 's/.*priority=//p' "$conf" | tail -n 1)
[ "$p1" -gt "$p2" ] || fail "the first network listed is not preferred"
grep -q -- '-i wlan0 -c' "$TMP/sup.log" || fail "supplicant not on wlan0"
grep -q -- '-i wlan0' "$TMP/dhcp.log" || fail "no DHCP on wlan0"
[ -e "$TMP/run/slot-wifi.on" ] || fail "up left no on mark"
[ "$(run status)" = 192.168.1.42 ] || fail "status did not print the address"

reset
printf 'ssid=Short\npsk=1234567\n' > "$TMP/wifi.conf"
run up && rc=0 || rc=$?
[ "$rc" = 3 ] || fail "a 7 character password exited $rc, want 3"

reset
printf 'ssid=Away\npsk=password1\n' > "$TMP/wifi.conf"
export FAKE_STATE=SCANNING
run up && rc=0 || rc=$?
unset FAKE_STATE
[ "$rc" = 4 ] || fail "no network in range exited $rc, want 4"
[ -e "$TMP/dhcp.log" ] && fail "DHCP ran without a network"

FAKE_LEASE=: reset
printf 'ssid=Home\npsk=password1\n' > "$TMP/wifi.conf"
run up && rc=0 || rc=$?
[ "$rc" = 5 ] || fail "no lease exited $rc, want 5"

reset
printf 'ssid=Home\npsk=password1\n' > "$TMP/wifi.conf"
run up >/dev/null
echo 1234 > "$TMP/run/slot-wifi-udhcpc.pid"
: > "$TMP/cli.log"
run pause && rc=0 || rc=$?
[ "$rc" = 0 ] || fail "pause exited $rc"
grep -q terminate "$TMP/cli.log" || fail "pause left the supplicant running"
[ -e "$TMP/run/slot-wifi.paused" ] || fail "pause left no mark"
[ -e "$TMP/run/slot-wifi-udhcpc.pid" ] && fail "pause left DHCP running"
: > "$TMP/sup.log"
run resume && rc=0 || rc=$?
[ "$rc" = 0 ] || fail "resume exited $rc"
grep -q -- '-i wlan0' "$TMP/sup.log" || fail "resume did not rejoin"
[ -e "$TMP/run/slot-wifi.paused" ] && fail "resume left the pause mark"

reset
: > "$TMP/cli.log"
run pause && rc=0 || rc=$?
[ "$rc" = 0 ] || fail "pause while off exited $rc"
[ -s "$TMP/cli.log" ] && fail "pause while off touched the supplicant"
run resume
[ -e "$TMP/sup.log" ] && fail "resume after an off pause started Wi-Fi"

reset
printf 'ssid=Home\npsk=password1\n' > "$TMP/wifi.conf"
run up >/dev/null
run down && rc=0 || rc=$?
[ "$rc" = 0 ] || fail "down exited $rc"
[ -e "$TMP/run/slot-wifi.on" ] && fail "down left the on mark"
run pause
[ -e "$TMP/run/slot-wifi.paused" ] && fail "a pause after down will turn Wi-Fi back on"

reset
run sideways 2>/dev/null && rc=0 || rc=$?
[ "$rc" = 2 ] || fail "bad verb exited $rc, want 2"

if [ "$FAILS" -eq 0 ]; then
	echo "wifi: all passed"
else
	echo "wifi: $FAILS failed"
	exit 1
fi
