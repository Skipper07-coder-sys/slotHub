#!/bin/sh
set -eu

HERE="$(cd "$(dirname "$0")" && pwd)"
SCRIPT="$HERE/System/update.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT HUP INT TERM
FAILS=0

fail() {
	echo "FAIL: $*"
	FAILS=$((FAILS + 1))
}

# A stand-in for GitHub: curl answers each URL with a file from $TMP/web, and
# writes down what it was asked for.
make_curl() {
	cat > "$TMP/bin/curl" <<FAKE
#!/bin/sh
out=
url=
while [ \$# -gt 0 ]; do
	case "\$1" in
	-o) out="\$2"; shift ;;
	-H | --connect-timeout | --max-time) shift ;;
	-*) ;;
	*) url="\$1" ;;
	esac
	shift
done
echo "\$url" >> "$TMP/curl.log"
f="$TMP/web/\$(printf '%s' "\$url" | sed 's|[^A-Za-z0-9._-]|_|g')"
[ -f "\$f" ] || exit 22
if [ -n "\$out" ]; then cp "\$f" "\$out"; else cat "\$f"; fi
FAKE
	chmod 755 "$TMP/bin/curl"
}

web() {
	printf '%s' "$1" | sed 's|[^A-Za-z0-9._-]|_|g'
}

release_json() {
	cat <<JSON
{
  "tag_name": "$1",
  "prerelease": false,
  "assets": [
    {
      "name": "x.txt",
      "browser_download_url": "https://dl/x.txt"
    },
    {
      "name": "$2",
      "browser_download_url": "https://dl/$2"
    },
    {
      "name": "SHA256SUMS",
      "browser_download_url": "https://dl/SHA256SUMS"
    }
  ]
}
JSON
}

reset() {
	rm -rf "$TMP/sd" "$TMP/bin" "$TMP/web" "$TMP"/*.log
	mkdir -p "$TMP/sd/System" "$TMP/bin" "$TMP/web"
	echo slot-binary > "$TMP/sd/System/slot"
	cp "$SCRIPT" "$TMP/sd/System/update.sh"
	printf 'v1.7.0\n' > "$TMP/sd/System/release"
	printf 'BASEOS_VERSION=1.3.0\nBASEOS_TARGET=rg34xxsp\n' > "$TMP/baseos-release"
	make_curl
	release_json v1.8.0 slot-v1.8.0-h700.zip > "$TMP/web/$(web 'https://api.github.com/repos/Skipper07-coder-sys/slotHub/releases/latest')"
	release_json v1.4.0 baseos-rg34xxsp-1.4.0.bosupd | sed 's|"x.txt"|"baseos-rg35xxsp-1.4.0.bosupd"|; s|https://dl/x.txt|https://dl/baseos-rg35xxsp-1.4.0.bosupd|' \
		> "$TMP/web/$(web 'https://api.github.com/repos/pvaibhav/BaseOS/releases/latest')"
	# The zip holds slot-v1.8.0/System/slot like a real release does.
	mkdir -p "$TMP/tree/slot-v1.8.0/System"
	echo new-slot-binary > "$TMP/tree/slot-v1.8.0/System/slot"
	cp "$SCRIPT" "$TMP/tree/slot-v1.8.0/System/update.sh"
	(cd "$TMP/tree" && zip -qr "$TMP/web/$(web 'https://dl/slot-v1.8.0-h700.zip')" slot-v1.8.0)
	rm -rf "$TMP/tree"
	(cd "$TMP/web" && cp "$(web 'https://dl/slot-v1.8.0-h700.zip')" slot-v1.8.0-h700.zip \
		&& sha256sum slot-v1.8.0-h700.zip > "$(web 'https://dl/SHA256SUMS')" && rm slot-v1.8.0-h700.zip)
	printf 'manifest\n' > "$TMP/manifest"
	tar -cf "$TMP/web/$(web 'https://dl/baseos-rg34xxsp-1.4.0.bosupd')" -C "$TMP" manifest
}

run() {
	PATH="$TMP/bin:$PATH" SLOT_ROOT="$TMP/sd" AGS_CURL="$TMP/bin/curl" \
		AGS_BASEOS_RELEASE="$TMP/baseos-release" AGS_UPDATE_LOG="$TMP/update.log" \
		sh "$TMP/sd/System/update.sh" "$@"
}

reset
out="$(run check)" && rc=0 || rc=$?
[ "$rc" = 0 ] || fail "check exited $rc"
for line in slot_installed=v1.7.0 slot_latest=v1.8.0 slot_zip=https://dl/slot-v1.8.0-h700.zip \
	slot_sums=https://dl/SHA256SUMS baseos_installed=1.3.0 baseos_latest=1.4.0 \
	baseos_url=https://dl/baseos-rg34xxsp-1.4.0.bosupd; do
	printf '%s\n' "$out" | grep -qx "$line" || fail "check did not print $line"
done

reset
rm "$TMP/web/$(web 'https://api.github.com/repos/Skipper07-coder-sys/slotHub/releases/latest')"
run check >/dev/null 2>&1 && rc=0 || rc=$?
[ "$rc" = 4 ] || fail "check with no GitHub exited $rc, want 4"

reset
echo beta > "$TMP/sd/update-channel"
release_json v1.9.0-rc1 slot-v1.9.0-rc1-h700.zip | sed 's/\[/[ /' > "$TMP/web/$(web 'https://api.github.com/repos/Skipper07-coder-sys/slotHub/releases?per_page=1')"
out="$(run check)" || fail "beta check failed"
printf '%s\n' "$out" | grep -qx 'slot_latest=v1.9.0-rc1' || fail "beta channel did not take the pre-release"

reset
run slot && rc=0 || rc=$?
[ "$rc" = 0 ] || fail "slot exited $rc"
[ "$(cat "$TMP/sd/System.new/slot")" = new-slot-binary ] || fail "System.new has no new slot"
[ "$(cat "$TMP/sd/System.new/release")" = v1.8.0 ] || fail "System.new is not stamped v1.8.0"
[ -e "$TMP/sd/System.new.tmp" ] && fail "slot left its temp dir"
[ "$(cat "$TMP/sd/System/slot")" = slot-binary ] || fail "slot replaced System before the swap"

reset
printf '0000000000000000000000000000000000000000000000000000000000000000  slot-v1.8.0-h700.zip\n' > "$TMP/web/$(web 'https://dl/SHA256SUMS')"
run slot && rc=0 || rc=$?
[ "$rc" = 5 ] || fail "a wrong checksum exited $rc, want 5"
[ -e "$TMP/sd/System.new" ] && fail "a wrong checksum still left System.new"

reset
rm "$TMP/web/$(web 'https://dl/slot-v1.8.0-h700.zip')"
run slot && rc=0 || rc=$?
[ "$rc" = 4 ] || fail "a failed download exited $rc, want 4"

reset
run baseos && rc=0 || rc=$?
[ "$rc" = 0 ] || fail "baseos exited $rc"
[ -f "$TMP/sd/baseos-rg34xxsp-1.4.0.bosupd" ] || fail "no payload at the card root"
grep -q rg35xxsp "$TMP/curl.log" && fail "baseos fetched another device's payload"

reset
printf 'BASEOS_VERSION=1.3.0\nBASEOS_TARGET=rg99\n' > "$TMP/baseos-release"
run baseos && rc=0 || rc=$?
[ "$rc" = 7 ] || fail "baseos with no payload for the device exited $rc, want 7"

# swap: a waiting System.new goes in, the old one stays as System.prev on trial
reset
mkdir -p "$TMP/sd/System.new" "$TMP/sd/System/Cache"
echo new-slot-binary > "$TMP/sd/System.new/slot"
echo cached > "$TMP/sd/System/Cache/x"
run swap || fail "swap exited $?"
[ "$(cat "$TMP/sd/System/slot")" = new-slot-binary ] || fail "swap did not put System.new in place"
[ "$(cat "$TMP/sd/System.prev/slot")" = slot-binary ] || fail "swap did not keep the old System"
[ "$(cat "$TMP/sd/System/.trial")" = 0 ] || fail "swap did not start a trial"
[ -f "$TMP/sd/System/Cache/x" ] || fail "swap dropped the label cache"
# the old update.sh is gone with the old System; the test copy stands in
cp "$SCRIPT" "$TMP/sd/System/update.sh"
run swap || fail "second swap exited $?"
[ "$(cat "$TMP/sd/System/.trial")" = 1 ] || fail "a start on trial was not counted"
run swap
run swap
[ "$(cat "$TMP/sd/System/slot")" = slot-binary ] || fail "three failed starts did not bring System.prev back"
[ -d "$TMP/sd/System.failed" ] || fail "the failed System was not kept for a look"
[ -e "$TMP/sd/System/.trial" ] && fail "the restored System is still on trial"

reset
mkdir -p "$TMP/sd/System.new" "$TMP/sd/System.prev"
echo new-slot-binary > "$TMP/sd/System.new/slot"
run swap || fail "swap exited $?"
cp "$SCRIPT" "$TMP/sd/System/update.sh"
run swap
run confirm || fail "confirm exited $?"
[ -e "$TMP/sd/System/.trial" ] && fail "confirm left the trial open"
[ -e "$TMP/sd/System.prev" ] && fail "confirm left System.prev"
run swap
[ "$(cat "$TMP/sd/System/slot")" = new-slot-binary ] || fail "a confirmed System was swapped away"

reset
run confirm || fail "confirm with nothing on trial exited $?"
run swap || fail "swap with nothing waiting exited $?"
[ "$(cat "$TMP/sd/System/slot")" = slot-binary ] || fail "an idle swap changed System"

reset
run sideways 2>/dev/null && rc=0 || rc=$?
[ "$rc" = 2 ] || fail "bad verb exited $rc, want 2"

if [ "$FAILS" -eq 0 ]; then
	echo "update: all passed"
else
	echo "update: $FAILS failed"
	exit 1
fi
