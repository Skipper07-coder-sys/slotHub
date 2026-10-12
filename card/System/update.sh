#!/bin/sh
# Updates for slot and BaseOS, fetched over Wi-Fi from their GitHub releases
# with the curl BaseOS ships. Verbs:
#   check            print what is installed and what the latest releases are
#   slot             download the latest slot release into System.new; the next
#                    start swaps it in (see launch_frontend.sh) and keeps the old
#                    System as System.prev until the new one has run
#   baseos           download the latest BaseOS payload to the card root; BaseOS
#                    applies it on the next restart and rolls back by itself
#                    Both print "progress P" (percent) lines while downloading,
#                    then "verifying" and, for slot, "unpacking".
#   swap             run by launch_frontend.sh before each start: move a
#                    downloaded System.new into place, and put System.prev back
#                    when a new System has failed to come up three times
#   confirm          run by slot once it is up: the new System stays, System.prev
#                    goes
# check prints key=value lines:
#   slot_installed=<tag>     from System/release, or "unknown"
#   slot_latest=<tag>        the newest non-prerelease, or "" when none
#   slot_zip=<url>           its h700 zip
#   slot_sums=<url>          its SHA256SUMS
#   slot_size=<bytes>        the zip's size, 0 when the reply has none
#   baseos_installed=<ver>   from /etc/baseos-release
#   baseos_latest=<ver>
#   baseos_url=<url>         the .bosupd for this device
#   baseos_size=<bytes>
# Exit 0 on success, 4 when GitHub could not be reached, 5 when a download did
# not match its checksum, 6 when the download could not be unpacked, 7 when
# there is nothing to install, 1 otherwise.
set -u

SD="${SLOT_ROOT:-/mnt/sdcard}"
SYS="$SD/System"
LOG="${AGS_UPDATE_LOG:-$SD/slot-update.log}"
CURL="${AGS_CURL:-curl}"
UNZIP="${AGS_UNZIP:-unzip}"
SHA256SUM="${AGS_SHA256SUM:-sha256sum}"
SLOT_REPO="${AGS_SLOT_REPO:-Skipper07-coder-sys/slotHub}"
BASEOS_REPO="${AGS_BASEOS_REPO:-pvaibhav/BaseOS}"
API="${AGS_GITHUB_API:-https://api.github.com}"
BASEOS_RELEASE="${AGS_BASEOS_RELEASE:-/etc/baseos-release}"
CHANNEL="${AGS_UPDATE_CHANNEL:-$SD/update-channel}"

log() {
	echo "$(date '+%H:%M:%S') $*" >> "$LOG" 2>/dev/null
	return 0
}

fetch() {
	$CURL -fsSL --connect-timeout 10 --max-time "${2:-30}" \
		-H 'Accept: application/vnd.github+json' -H 'User-Agent: slot-update' "$1"
}

file_size() {
	[ -f "$1" ] || { echo 0; return 0; }
	stat -c %s "$1" 2>/dev/null || wc -c < "$1"
}

# download URL FILE [BYTES]: with the expected size, prints "progress P" each
# time the percent on the card moves while curl runs.
download() {
	$CURL -fsSL --connect-timeout 10 --max-time 900 -H 'User-Agent: slot-update' -o "$2" "$1" &
	pid=$!
	# If slot goes away mid-download the next progress line kills this
	# script; curl must not go on filling the card on its own.
	trap 'kill "$pid" 2>/dev/null' EXIT HUP INT TERM PIPE
	total="${3:-0}"
	last=-1
	while kill -0 "$pid" 2>/dev/null; do
		if [ "$total" -gt 0 ] 2>/dev/null; then
			pct=$(( $(file_size "$2") * 100 / total ))
			[ "$pct" -gt 99 ] && pct=99
			[ "$pct" -ne "$last" ] && echo "progress $pct" && last=$pct
		fi
		sleep 1
	done
	wait "$pid"
	rc=$?
	trap - EXIT HUP INT TERM PIPE
	return $rc
}

# The release API answers in JSON, minified or pretty-printed; BusyBox has no
# jq, so the reply is cut at every comma and bracket, leaving one key per line
# in the order GitHub sent them, and fields are picked out with sed.
keys() {
	tr ',{}[]' '\n\n\n\n\n'
}

json_field() {
	keys | sed -n "s/^[[:space:]]*\"$1\":[[:space:]]*\"\([^\"]*\)\".*/\1/p" | head -n 1
}

asset_url() {
	keys | sed -n 's/^[[:space:]]*"browser_download_url":[[:space:]]*"\([^"]*\)".*/\1/p' | grep -- "$1" | head -n 1
}

# An asset's "size" comes a few keys before its browser_download_url.
asset_size() {
	keys | sed -n -e 's/^[[:space:]]*"size":[[:space:]]*\([0-9]*\).*/size \1/p' \
		-e 's/^[[:space:]]*"browser_download_url":[[:space:]]*"\([^"]*\)".*/url \1/p' |
		while read -r kind value; do
			case "$kind" in
			size) size="$value" ;;
			url) [ "$value" = "$1" ] && echo "${size:-0}" && break ;;
			esac
		done
}

# Pre-releases are opt-in: a file named update-channel at the card root
# containing "beta" takes the newest release of any kind.
latest_release() {
	if [ "$(cat "$CHANNEL" 2>/dev/null | tr -d ' \r\n')" = beta ]; then
		fetch "$API/repos/$1/releases?per_page=1" 2>/dev/null
	else
		fetch "$API/repos/$1/releases/latest" 2>/dev/null
	fi
}

baseos_target() {
	sed -n 's/^BASEOS_TARGET=//p' "$BASEOS_RELEASE" 2>/dev/null | head -n 1
}

baseos_version() {
	sed -n 's/^BASEOS_VERSION=//p' "$BASEOS_RELEASE" 2>/dev/null | head -n 1
}

do_check() {
	slot_installed="$(tr -d ' \r\n' < "$SYS/release" 2>/dev/null)"
	echo "slot_installed=${slot_installed:-unknown}"
	echo "baseos_installed=$(baseos_version)"
	reply="$(latest_release "$SLOT_REPO")" || {
		log "check: could not reach GitHub for $SLOT_REPO"
		return 4
	}
	tag="$(printf '%s\n' "$reply" | json_field tag_name)"
	[ -n "$tag" ] || log "check: no tag_name in the reply for $SLOT_REPO: $(printf '%s' "$reply" | head -c 200)"
	echo "slot_latest=$tag"
	zip="$(printf '%s\n' "$reply" | asset_url '-h700.zip')"
	echo "slot_zip=$zip"
	echo "slot_sums=$(printf '%s\n' "$reply" | asset_url 'SHA256SUMS')"
	size="$(printf '%s\n' "$reply" | asset_size "$zip")"
	echo "slot_size=${size:-0}"
	target="$(baseos_target)"
	reply="$(latest_release "$BASEOS_REPO")" || {
		log "check: could not reach GitHub for $BASEOS_REPO"
		echo "baseos_latest="
		echo "baseos_url="
		echo "baseos_size=0"
		return 0
	}
	echo "baseos_latest=$(printf '%s\n' "$reply" | json_field tag_name | sed 's/^v//')"
	url=""
	[ -n "$target" ] && url="$(printf '%s\n' "$reply" | asset_url "baseos-$target-.*\.bosupd")"
	echo "baseos_url=$url"
	size=""
	[ -n "$url" ] && size="$(printf '%s\n' "$reply" | asset_size "$url")"
	echo "baseos_size=${size:-0}"
	return 0
}

# SHA256SUMS lists "<hash>  <file>" per line, as sha256sum prints it.
verify() {
	want="$(grep -- " $(basename "$2")\$" "$3" | head -n 1 | cut -d' ' -f1)"
	[ -n "$want" ] || { log "no checksum for $(basename "$2")"; return 5; }
	have="$($SHA256SUM "$1" | cut -d' ' -f1)"
	[ "$have" = "$want" ] || { log "checksum mismatch for $(basename "$2")"; return 5; }
	return 0
}

do_slot() {
	tmp="$SD/System.new.tmp"
	/bin/rm -rf "$tmp" "$SD/System.new"
	/bin/mkdir -p "$tmp"
	check="$(do_check)" || { /bin/rm -rf "$tmp"; return 4; }
	zip="$(printf '%s\n' "$check" | sed -n 's/^slot_zip=//p')"
	sums="$(printf '%s\n' "$check" | sed -n 's/^slot_sums=//p')"
	tag="$(printf '%s\n' "$check" | sed -n 's/^slot_latest=//p')"
	size="$(printf '%s\n' "$check" | sed -n 's/^slot_size=//p')"
	[ -n "$zip" ] && [ -n "$sums" ] || { log "slot: release $tag has no zip or checksums"; /bin/rm -rf "$tmp"; return 7; }
	log "slot: downloading $tag"
	download "$zip" "$tmp/slot.zip" "$size" && download "$sums" "$tmp/SHA256SUMS" || {
		log "slot: download failed"
		/bin/rm -rf "$tmp"
		return 4
	}
	echo verifying
	verify "$tmp/slot.zip" "$zip" "$tmp/SHA256SUMS" || { rc=$?; /bin/rm -rf "$tmp"; return $rc; }
	echo unpacking
	$UNZIP -q "$tmp/slot.zip" -d "$tmp/tree" >/dev/null 2>&1 || {
		log "slot: unzip failed"
		/bin/rm -rf "$tmp"
		return 6
	}
	new="$(find "$tmp/tree" -type d -name System | head -n 1)"
	# Whatever goes in has to be able to start, and to put the old one back.
	for f in slot launch_frontend.sh update.sh; do
		[ -n "$new" ] && [ -f "$new/$f" ] || { log "slot: no System/$f in the zip"; /bin/rm -rf "$tmp"; return 6; }
	done
	printf '%s\n' "$tag" > "$new/release"
	mv "$new" "$SD/System.new" || { /bin/rm -rf "$tmp"; return 1; }
	/bin/rm -rf "$tmp"
	sync
	log "slot: $tag ready in System.new"
	return 0
}

do_baseos() {
	check="$(do_check)" || return 4
	url="$(printf '%s\n' "$check" | sed -n 's/^baseos_url=//p')"
	ver="$(printf '%s\n' "$check" | sed -n 's/^baseos_latest=//p')"
	size="$(printf '%s\n' "$check" | sed -n 's/^baseos_size=//p')"
	[ -n "$url" ] || { log "baseos: no payload for this device"; return 7; }
	name="$(basename "$url")"
	/bin/rm -f "$SD"/baseos-*.bosupd.part
	log "baseos: downloading $ver"
	download "$url" "$SD/$name.part" "$size" || { log "baseos: download failed"; /bin/rm -f "$SD/$name.part"; return 4; }
	# The payload carries its own manifest hash, which BaseOS checks after
	# writing the slot; here make sure the file at least arrived whole.
	echo verifying
	if ! tar -tf "$SD/$name.part" >/dev/null 2>&1; then
		log "baseos: payload is not a readable archive"
		/bin/rm -f "$SD/$name.part"
		return 5
	fi
	# Only now is an older payload let go of.
	for old in "$SD"/baseos-*.bosupd; do
		[ "$old" = "$SD/$name" ] || /bin/rm -f "$old"
	done
	mv "$SD/$name.part" "$SD/$name" || { /bin/rm -f "$SD/$name.part"; return 1; }
	sync
	log "baseos: $name ready; applies on the next restart"
	return 0
}

TRIAL="$SYS/.trial"
TRIES="${AGS_UPDATE_TRIES:-3}"
LDSO="${AGS_LDSO:-/lib/ld-linux-aarch64.so.1}"

# splash DIR MESSAGE: the slot in DIR draws MESSAGE over the boot logo; it
# stays up until slot shows its first frame. Builds from before the splash
# would start the whole frontend instead, so only a slot that knows the flag
# is asked. -F: BusyBox grep without it stops at the binary's first NUL.
splash() {
	[ -f "$1/slot" ] && grep -qF -- '--splash' "$1/slot" 2>/dev/null || return 0
	# A splash that hangs must not hold up the start it is announcing.
	run=""
	command -v timeout >/dev/null 2>&1 && run="timeout 5"
	if [ -x "$LDSO" ]; then
		$run "$LDSO" "$1/slot" --splash "$2" >/dev/null 2>&1
	else
		$run "$1/slot" --splash "$2" >/dev/null 2>&1
	fi
	return 0
}

tag_of() {
	tr -d ' \r\n' < "$1/release" 2>/dev/null
}

do_swap() {
	if [ -f "$SD/System.new/slot" ]; then
		tag="$(tag_of "$SD/System.new")"
		splash "$SD/System.new" "Installing ${tag:-update}"
		/bin/rm -rf "$SD/System.prev"
		sync
		if mv "$SYS" "$SD/System.prev" && mv "$SD/System.new" "$SYS"; then
			# The label cache is worth keeping; a fresh one takes a while to build.
			[ -d "$SD/System.prev/Cache" ] && [ ! -d "$SYS/Cache" ] \
				&& mv "$SD/System.prev/Cache" "$SYS/Cache"
			echo 0 > "$TRIAL"
			sync
			log "swap: new System in place, on trial"
		else
			log "swap: could not move System.new into place"
			[ -d "$SYS" ] || mv "$SD/System.prev" "$SYS"
		fi
		return 0
	fi
	[ -f "$TRIAL" ] || return 0
	n="$(tr -d ' \r\n' < "$TRIAL" 2>/dev/null)"
	n=$(( ${n:-0} + 1 ))
	if [ "$n" -ge "$TRIES" ] && [ -f "$SD/System.prev/slot" ]; then
		tag="$(tag_of "$SD/System.prev")"
		splash "$SD/System.prev" "Restoring ${tag:-the last version}"
		/bin/rm -rf "$SD/System.failed"
		mv "$SYS" "$SD/System.failed" \
			&& { mv "$SD/System.prev" "$SYS" || mv "$SD/System.failed" "$SYS"; }
		/bin/rm -f "$SYS/.trial"
		sync
		log "swap: new System failed $n starts, System.prev is back"
		return 0
	fi
	echo "$n" > "$TRIAL"
	return 0
}

do_confirm() {
	[ -f "$TRIAL" ] || return 0
	/bin/rm -f "$TRIAL"
	/bin/rm -rf "$SD/System.prev" "$SD/System.failed"
	sync
	log "confirm: $(cat "$SYS/release" 2>/dev/null | tr -d ' \r\n') is in"
	return 0
}

case "${1:-}" in
check) do_check ;;
swap) do_swap ;;
confirm) do_confirm ;;
slot) do_slot ;;
baseos) do_baseos ;;
*)
	echo "usage: update.sh [check|slot|baseos|swap|confirm]" >&2
	exit 2
	;;
esac
