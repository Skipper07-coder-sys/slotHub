#!/bin/sh
set -eu

HERE="$(cd "$(dirname "$0")" && pwd)"
SCRIPT="$HERE/System/report.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT HUP INT TERM
FAILS=0

fail() {
	echo "FAIL: $*"
	FAILS=$((FAILS + 1))
}

CARD="$TMP/card"
run() {
	SLOT_ROOT="$CARD" AGS_PSY="$TMP/psy" AGS_NET_SYS="$TMP/net" AGS_BT_HCI="$TMP/hci0" \
		AGS_FB="$TMP/fb0" AGS_INPUT="$TMP/input" AGS_ASOUND="$TMP/cards" AGS_MIN_FREE_KB=1 \
		sh "$SCRIPT" "$@"
}

# A healthy handheld: every device there, System matching its manifest.
mkdir -p "$CARD/System" "$TMP/psy" "$TMP/net/wlan0" "$TMP/hci0" "$TMP/input"
echo v9.9.9 > "$CARD/System/release"
for f in slot mgba_libretro.so gpsp_libretro.so; do echo "$f" > "$CARD/System/$f"; done
(cd "$CARD/System" && sha256sum release slot mgba_libretro.so gpsp_libretro.so > MANIFEST.sha256)
echo 80 > "$TMP/psy/capacity"
: > "$TMP/fb0"
: > "$TMP/input/event0"
echo ' 0 [audiocodec     ]: audiocodec - audiocodec' > "$TMP/cards"

out="$(SLOT_WIFI=1 SLOT_BT=1 run selftest)" && rc=0 || rc=$?
[ "$rc" = 0 ] || fail "a healthy self-test exited $rc, want 0"
[ "$out" = "problems 0" ] || fail "a healthy self-test printed '$out'"
grep -q '^ok System files match the release' "$CARD/selftest.log" || fail "the manifest was not checked"
grep -q '^ok Wi-Fi' "$CARD/selftest.log" || fail "Wi-Fi was not checked when on"
head -n 1 "$CARD/selftest.log" | grep -q ' slot v9.9.9$' || fail "the log does not name the version"

# Once per version: a second run for the same version does nothing.
out="$(run selftest-once)" && rc=0 || rc=$?
[ "$rc" = 0 ] && [ -z "$out" ] || fail "selftest-once ran again for the same version ($rc, '$out')"
echo v9.9.10 > "$CARD/System/release"
(cd "$CARD/System" && sha256sum release slot mgba_libretro.so gpsp_libretro.so > MANIFEST.sha256)
out="$(run selftest-once)" || true
[ "$out" = "problems 0" ] || fail "selftest-once skipped a new version ('$out')"

# Problems: a changed core, no sound card, Wi-Fi on but no wlan0.
echo corrupt > "$CARD/System/gpsp_libretro.so"
: > "$TMP/cards"
rmdir "$TMP/net/wlan0"
out="$(SLOT_WIFI=1 run selftest)" && rc=0 || rc=$?
[ "$rc" = 3 ] || fail "a failing self-test exited $rc, want 3"
[ "$out" = "problems 3" ] || fail "a failing self-test printed '$out', want problems 3"
grep -q 'gpsp_libretro.so' "$CARD/selftest.log" || fail "the changed file is not named"
out="$(SLOT_WIFI=0 run selftest)" || true
[ "$out" = "problems 2" ] || fail "Wi-Fi was checked while off ('$out')"

# A bug report: logs scrubbed of secrets, no Wi-Fi config or saves, old ones pruned.
cat > "$CARD/slot-wifi.log" <<'LOG'
12:00:00 joining home psk=hunter2
12:00:01 api_key: abc123 sent
LOG
cat > "$CARD/slot.log" <<'LOG'
slot: cheevos: {"Success":true,"User":"me","Token":"rAtOkEn42","Score":0}
slot: cheevos: {"Password": "raPassw0rd"}
slot: cheevos: r=login2&u=me&p=formPass99&k=1
slot: cheevos: r=awardachievement&u=me&t=formTok77&a=1
LOG
echo 'network={ psk="hunter2" }' > "$CARD/wifi.conf"
mkdir -p "$CARD/Saves" "$CARD/crash"
echo save > "$CARD/Saves/game.sav"
echo "pid=1 signal=11" > "$CARD/crash/cores.log"
echo big > "$CARD/crash/slot.core"
out="$(run bugreport)" && rc=0 || rc=$?
[ "$rc" = 0 ] || fail "bugreport exited $rc"
name="${out#saved }"
dir="$CARD/Reports/$name"
[ -d "$dir" ] || fail "no report folder for '$out'"
[ -f "$dir/slot-wifi.log" ] || fail "the logs were not copied"
grep -q hunter2 "$dir/slot-wifi.log" && fail "a Wi-Fi password reached the report"
grep -q abc123 "$dir/slot-wifi.log" && fail "an API key reached the report"
grep -rq hunter2 "$dir" && fail "something secret reached the report"
[ -f "$dir/slot.log" ] || fail "slot.log was not copied"
for secret in rAtOkEn42 raPassw0rd formPass99 formTok77; do
	grep -rq "$secret" "$dir" && fail "a RetroAchievements secret ($secret) reached the report"
done
grep -q 'r=login2&u=me&p=\[removed\]&k=1' "$dir/slot.log" || fail "the form scrub took more than the password"
[ -e "$dir/game.sav" ] || [ -e "$dir/Saves" ] && fail "saves reached the report"
[ -f "$dir/crash/cores.log" ] || fail "the crash notes were not copied"
[ -e "$dir/crash/slot.core" ] && fail "a core dump was copied"
[ -f "$dir/system.txt" ] || fail "no system snapshot"
[ -f "$dir/selftest.log" ] || [ -f "$CARD/selftest.log" ] || fail "no self-test ran"
for i in 1 2 3 4 5 6; do mkdir -p "$CARD/Reports/bugreport-2000-01-0${i}_00-00-00"; touch -d "2000-01-0$i" "$CARD/Reports/bugreport-2000-01-0${i}_00-00-00"; done
run bugreport > /dev/null
[ "$(ls -d "$CARD"/Reports/*/ | wc -l | tr -d ' ')" = 5 ] || fail "old reports were not pruned to 5"
[ -d "$CARD/Reports/$name" ] || fail "pruning removed a new report"

# The crash catcher: only a crashed run leaves a note, and it is a report.
rm -rf "$CARD/Reports" "$CARD/crash/caught"
echo "slot: started" > "$CARD/slot.log"
run crashcheck
[ -e "$CARD/crash/caught" ] && fail "a clean run left a crash note"
[ -d "$CARD/Reports" ] && fail "a clean run saved a report"
printf 'slot: started\nslot: crash: SIGSEGV in thread main addr 0x0 pc 0x1\n' > "$CARD/slot.log"
run crashcheck
[ -f "$CARD/crash/caught" ] || fail "a crashed run left no note"
ls -d "$CARD"/Reports/crash-*/ > /dev/null 2>&1 || fail "a crashed run saved no report"
grep -q "SIGSEGV" "$CARD"/Reports/crash-*/slot.log || fail "the crash report lacks the crashed log"
rm -rf "$CARD/Reports" "$CARD/crash/caught"
printf "thread 'main' panicked at src/app.rs:1:1:\n" > "$CARD/slot.log"
run crashcheck
[ -f "$CARD/crash/caught" ] || fail "a panic was not caught"

if [ "$FAILS" -gt 0 ]; then
	echo "$FAILS failure(s)"
	exit 1
fi
echo "report: all passed"
