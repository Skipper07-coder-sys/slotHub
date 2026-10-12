#!/bin/sh
set -eu

HERE="$(cd "$(dirname "$0")" && pwd)"
SCRIPT="$HERE/System/drive.sh"
TMP="$(mktemp -d)"
trap 'sh "$SCRIPT" stop >/dev/null 2>&1 || true; rm -rf "$TMP"' EXIT HUP INT TERM
FAILS=0

fail() {
	echo "FAIL: $*"
	FAILS=$((FAILS + 1))
}

# A stand-in File Browser: setup verbs return at once, the server stays up
# until killed (or dies at once when FAKE_DIE is set).
fake() {
	cat > "$TMP/filebrowser" <<FAKE
#!/bin/sh
printf '%s\n' "\$*" >> "$TMP/fb.log"
case "\$*" in
*"config init"*|*"users add"*) exit 0 ;;
esac
[ -n "\${FAKE_DIE:-}" ] && exit 1
exec sleep 30
FAKE
	chmod 755 "$TMP/filebrowser"
}

run() {
	SLOT_ROOT="$TMP/card" AGS_FILEBROWSER="$TMP/filebrowser" AGS_DRIVE_RUN="$TMP/run" \
		sh "$SCRIPT" "$@"
}

mkdir -p "$TMP/card"

run start && rc=0 || rc=$?
[ "$rc" = 3 ] || fail "start with no server exited $rc, want 3"

fake
run start && rc=0 || rc=$?
[ "$rc" = 0 ] || fail "start exited $rc, want 0"
grep -q -- "config init --auth.method=noauth" "$TMP/fb.log" || fail "no login was not set up"
grep -q -- "--perm.execute=false" "$TMP/fb.log" || fail "the user may run commands"
grep -q -- "-a 0.0.0.0 -p 80 -r $TMP/card" "$TMP/fb.log" || fail "the card is not served on port 80"
pid="$(cat "$TMP/run/pid")"
kill -0 "$pid" 2>/dev/null || fail "the server is not running after start"

run start && rc=0 || rc=$?
[ "$rc" = 0 ] || fail "a second start exited $rc, want 0"
kill -0 "$pid" 2>/dev/null && fail "a second start left the first server running"

run stop && rc=0 || rc=$?
[ "$rc" = 0 ] || fail "stop exited $rc, want 0"
[ -e "$TMP/run/pid" ] && fail "stop left the pid file"
run stop || fail "stop with nothing running failed"

FAKE_DIE=1 run start && rc=0 || rc=$?
[ "$rc" = 1 ] || fail "a server that dies at start exited $rc, want 1"

if [ "$FAILS" -gt 0 ]; then
	echo "$FAILS failure(s)"
	exit 1
fi
echo "drive.sh: all passed"
