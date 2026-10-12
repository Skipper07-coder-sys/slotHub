#!/bin/sh
# Wi-Fi Drive: serves the card to a web browser on the home network, so games
# and labels can be dropped on from a computer or phone. The server is File
# Browser (filebrowser.org, Apache-2.0), the same one muOS ships for this job;
# the release build puts it at System/bin/filebrowser. Verbs:
#   start   serve the card at http://<hostname>.local until stopped
#   stop    stop serving
# Only slot turns it on, from Wi-Fi > Wi-Fi Drive, and it never outlives Wi-Fi
# or a restart (its state lives in /tmp). Anyone on the same network can open
# it while it is on: there is no password, as with BaseOS's own SFTP login.
# Exit 0 on success, 3 when the server is not on the card, 1 otherwise.
set -u

SD="${SLOT_ROOT:-/mnt/sdcard}"
SYS="$SD/System"
FB="${AGS_FILEBROWSER:-$SYS/bin/filebrowser}"
RUN="${AGS_DRIVE_RUN:-/tmp/slot-drive}"
PORT="${AGS_DRIVE_PORT:-80}"
LOG="$RUN/drive.log"
PIDFILE="$RUN/pid"

log() {
	echo "$(date '+%H:%M:%S') $*" >> "$LOG" 2>/dev/null
	return 0
}

running() {
	pid="$(cat "$PIDFILE" 2>/dev/null)"
	[ -n "$pid" ] && kill -0 "$pid" 2>/dev/null
}

stop() {
	running || { rm -f "$PIDFILE"; return 0; }
	kill "$pid" 2>/dev/null
	i=0
	while running && [ "$i" -lt 30 ]; do
		sleep 0.1
		i=$((i + 1))
	done
	running && kill -9 "$pid" 2>/dev/null
	rm -f "$PIDFILE"
	log "stopped"
}

start() {
	mkdir -p "$RUN" || return 1
	stop
	if [ ! -x "$FB" ]; then
		log "no server at $FB"
		return 3
	fi
	# A fresh settings file each time: no login, one user that may not run
	# commands on the device.
	db="$RUN/drive.db"
	rm -f "$db"
	"$FB" -d "$db" config init --auth.method=noauth >> "$LOG" 2>&1 || return 1
	pass="$(tr -dc 'A-Za-z0-9' < /dev/urandom 2>/dev/null | head -c 24)"
	"$FB" -d "$db" users add slot "${pass:-slot-drive-$$-unused}" \
		--perm.execute=false >> "$LOG" 2>&1 || return 1
	"$FB" -d "$db" -a 0.0.0.0 -p "$PORT" -r "$SD" >> "$LOG" 2>&1 &
	echo $! > "$PIDFILE"
	# Give it a moment to bind the port: a server that dies at once (the port
	# taken, a bad binary) should read as a failure, not as on.
	sleep 1
	if ! running; then
		log "server exited at start"
		rm -f "$PIDFILE"
		return 1
	fi
	log "serving $SD on port $PORT"
}

case "${1:-}" in
start) start ;;
stop) stop ;;
*)
	echo "usage: $0 start|stop" >&2
	exit 1
	;;
esac
