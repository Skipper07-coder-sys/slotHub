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
		AGS_IP="$TMP/bin/ip" AGS_UDHCPC="$TMP/bin/udhcpc" AGS_IW="$TMP/bin/iw" \
		AGS_WIFI_JOIN_S=1 AGS_WIFI_ADDR_S=1 AGS_WIFI_IFACE_S=${IFACE_S:-1} \
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

# wpa_cli as a supplicant would answer: a table of what is in range.
scan_fake() {
	cat > "$TMP/bin/wpa_cli" <<FAKE
#!/bin/sh
printf '%s\\n' "\$*" >> "$TMP/cli.log"
case "\$*" in
*ping*) [ -n "\${FAKE_PONG:-}" ] && echo PONG ;;
*scan_results*)
	printf 'bssid / frequency / signal level / flags / ssid\\n'
	printf 'aa:01\\t2412\\t-71\\t[WPA2-PSK-CCMP][ESS]\\tHome\\n'
	printf 'aa:02\\t5180\\t-48\\t[WPA2-PSK-CCMP][ESS]\\tHome\\n'
	printf 'aa:03\\t2437\\t-80\\t[ESS]\\tCafe Free\\n'
	printf 'aa:04\\t2462\\t-60\\t[WPA2-PSK-CCMP][ESS]\\t\\n'
	;;
esac
exit 0
FAKE
	chmod 755 "$TMP/bin/wpa_cli"
}

reset
scan_fake
out="$(AGS_WIFI_SCAN_S=0 run scan)" && rc=0 || rc=$?
[ "$rc" = 0 ] || fail "scan exited $rc"
want="$(printf -- '-48\t1\tHome\n-80\t0\tCafe Free')"
[ "$out" = "$want" ] || fail "scan printed $(printf '%s' "$out" | od -c | head -3)"
grep -q -- '-i wlan0 -c' "$TMP/sup.log" || fail "scan with Wi-Fi off started no supplicant"
grep -q terminate "$TMP/cli.log" || fail "scan with Wi-Fi off left its supplicant running"

reset
scan_fake
FAKE_PONG=1 AGS_WIFI_SCAN_S=0 run scan >/dev/null
[ -e "$TMP/sup.log" ] && fail "scan started a second supplicant beside a running one"
grep -q terminate "$TMP/cli.log" && fail "scan stopped the supplicant Wi-Fi is using"

# The list fills a few seconds after the scan is asked for, as it does on the
# handheld (4 to 5 s); the script keeps reading without asking again.
reset
scan_fake
cat > "$TMP/bin/wpa_cli" <<FAKE
#!/bin/sh
printf '%s\\n' "\$*" >> "$TMP/cli.log"
case "\$*" in
*ping*) echo PONG ;;
*scan_results*)
	printf 'bssid / frequency / signal level / flags / ssid\\n'
	[ "\$(grep -c scan_results "$TMP/cli.log")" -ge 3 ] && printf 'aa:01\\t2412\\t-71\\t[WPA2-PSK-CCMP][ESS]\\tHome\\n'
	;;
esac
exit 0
FAKE
chmod 755 "$TMP/bin/wpa_cli"
out="$(AGS_WIFI_SCAN_S=0 run scan)" || fail "retried scan exited $?"
[ "$out" = "$(printf -- '-71\t1\tHome')" ] || fail "a scan whose list filled late was read too early"
[ "$(grep -c 'wlan0 scan$' "$TMP/cli.log")" = 1 ] || fail "a scan that was still running was asked for $(grep -c 'wlan0 scan$' "$TMP/cli.log") times, want 1"

reset
scan_fake
sed -i 's/^\*scan_results\*)/*scan_results*) : ;;\n*never*)/' "$TMP/bin/wpa_cli"
out="$(AGS_WIFI_SCAN_WAIT_S=4 run scan)" || fail "empty scan exited $?"
[ -z "$out" ] || fail "an empty scan printed something"
[ "$(grep -c 'wlan0 scan$' "$TMP/cli.log")" = 2 ] || fail "an empty scan was asked for $(grep -c 'wlan0 scan$' "$TMP/cli.log") times, want 2 (once more halfway)"
grep -q "scan: 0 results after 4s" "$TMP/slot-wifi.log" || fail "an empty scan was not logged"

# The radio can come up a few seconds after slot does; a scan waits for it.
reset
scan_fake
rm -rf "$TMP/net/wlan0"
(sleep 1; mkdir -p "$TMP/net/wlan0") &
out="$(IFACE_S=5 AGS_WIFI_SCAN_S=0 run scan)" || fail "scan before wlan0 exited $?"
wait
[ "$out" = "$(printf -- '-48\t1\tHome\n-80\t0\tCafe Free')" ] || fail "a scan did not wait for wlan0"

reset
rm -rf "$TMP/net/wlan0"
run scan >/dev/null 2>&1 && rc=0 || rc=$?
[ "$rc" = 1 ] || fail "scan with no wlan0 at all exited $rc, want 1"
grep -q "scan: no wlan0" "$TMP/slot-wifi.log" || fail "a scan with no wlan0 was not logged"

# With iw on the system, scanning goes through it: busy on the first pass, as
# the radio is right after coming up, then a list.
reset
cat > "$TMP/bin/iw" <<FAKE
#!/bin/sh
printf '%s\\n' "\$*" >> "$TMP/iw.log"
case "\$*" in
*scan)
	[ "\$(grep -c scan "$TMP/iw.log")" -ge 2 ] || { echo 'command failed: Device or resource busy (-16)' >&2; exit 240; }
	printf 'BSS aa:01(on wlan0)\\n\\tcapability: ESS Privacy (0x0411)\\n\\tsignal: -71.00 dBm\\n\\tSSID: Home\\n\\tRSN:\\t * Version: 1\\n'
	printf 'BSS aa:02(on wlan0)\\n\\tcapability: ESS Privacy (0x0411)\\n\\tsignal: -48.00 dBm\\n\\tSSID: Home\\n'
	printf 'BSS aa:03(on wlan0)\\n\\tcapability: ESS (0x0401)\\n\\tsignal: -80.00 dBm\\n\\tSSID: Cafe Free\\n'
	printf 'BSS aa:04(on wlan0)\\n\\tcapability: ESS Privacy (0x0411)\\n\\tsignal: -60.00 dBm\\n\\tSSID: \\n'
	;;
esac
exit 0
FAKE
chmod 755 "$TMP/bin/iw"
out="$(AGS_WIFI_SCAN_S=0 run scan)" || fail "iw scan exited $?"
[ "$out" = "$(printf -- '-48\t1\tHome\n-80\t0\tCafe Free')" ] || fail "iw scan printed $(printf '%s' "$out" | od -c | head -3)"
grep -q 'power_save off' "$TMP/iw.log" || fail "iw scan left power saving on"
[ -e "$TMP/sup.log" ] && fail "iw scan started a supplicant"
grep -q 'results on pass 2 (iw)' "$TMP/slot-wifi.log" || fail "a busy first pass was not retried"

reset
run sideways 2>/dev/null && rc=0 || rc=$?
[ "$rc" = 2 ] || fail "bad verb exited $rc, want 2"

if [ "$FAILS" -eq 0 ]; then
	echo "wifi: all passed"
else
	echo "wifi: $FAILS failed"
	exit 1
fi
