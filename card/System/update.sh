#!/bin/sh
# Updates for slot and BaseOS, fetched over Wi-Fi from their GitHub releases
# with the curl BaseOS ships. Verbs:
#   check            print what is installed and what the latest releases are
#   slot             download the latest slot release into System.new; the next
#                    start swaps it in (see launch_frontend.sh) and keeps the old
#                    System as System.prev until the new one has run
#   baseos           download the latest BaseOS payload to the card root; BaseOS
#                    applies it on the next restart and rolls back by itself
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
#   baseos_installed=<ver>   from /etc/baseos-release
#   baseos_latest=<ver>
#   baseos_url=<url>         the .bosupd for this device
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

download() {
	$CURL -fsSL --connect-timeout 10 --max-time 900 -H 'User-Agent: slot-update' -o "$2" "$1"
}

# The release API answers in JSON; BusyBox has no jq, so pick fields out with
# sed. Each asset's URL sits on its own line in GitHub's pretty-printed reply.
json_field() {
	sed -n "s/^[[:space:]]*\"$1\":[[:space:]]*\"\([^\"]*\)\".*/\1/p" | head -n 1
}

asset_url() {
	sed -n 's/^[[:space:]]*"browser_download_url":[[:space:]]*"\([^"]*\)".*/\1/p' | grep -- "$1" | head -n 1
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
	echo "slot_latest=$(printf '%s\n' "$reply" | json_field tag_name)"
	echo "slot_zip=$(printf '%s\n' "$reply" | asset_url '-h700.zip')"
	echo "slot_sums=$(printf '%s\n' "$reply" | asset_url 'SHA256SUMS')"
	target="$(baseos_target)"
	reply="$(latest_release "$BASEOS_REPO")" || {
		log "check: could not reach GitHub for $BASEOS_REPO"
		echo "baseos_latest="
		echo "baseos_url="
		return 0
	}
	echo "baseos_latest=$(printf '%s\n' "$reply" | json_field tag_name | sed 's/^v//')"
	if [ -n "$target" ]; then
		echo "baseos_url=$(printf '%s\n' "$reply" | asset_url "baseos-$target-.*\.bosupd")"
	else
		echo "baseos_url="
	fi
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
	[ -n "$zip" ] && [ -n "$sums" ] || { log "slot: release $tag has no zip or checksums"; /bin/rm -rf "$tmp"; return 7; }
	log "slot: downloading $tag"
	download "$zip" "$tmp/slot.zip" && download "$sums" "$tmp/SHA256SUMS" || {
		log "slot: download failed"
		/bin/rm -rf "$tmp"
		return 4
	}
	verify "$tmp/slot.zip" "$zip" "$tmp/SHA256SUMS" || { rc=$?; /bin/rm -rf "$tmp"; return $rc; }
	$UNZIP -q "$tmp/slot.zip" -d "$tmp/tree" >/dev/null 2>&1 || {
		log "slot: unzip failed"
		/bin/rm -rf "$tmp"
		return 6
	}
	new="$(find "$tmp/tree" -type d -name System | head -n 1)"
	[ -n "$new" ] && [ -f "$new/slot" ] || { log "slot: no System/slot in the zip"; /bin/rm -rf "$tmp"; return 6; }
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
	[ -n "$url" ] || { log "baseos: no payload for this device"; return 7; }
	name="$(basename "$url")"
	/bin/rm -f "$SD"/baseos-*.bosupd "$SD"/baseos-*.bosupd.part
	log "baseos: downloading $ver"
	download "$url" "$SD/$name.part" || { log "baseos: download failed"; /bin/rm -f "$SD/$name.part"; return 4; }
	# The payload carries its own manifest hash, which BaseOS checks after
	# writing the slot; here make sure the file at least arrived whole.
	if ! tar -tf "$SD/$name.part" >/dev/null 2>&1; then
		log "baseos: payload is not a readable archive"
		/bin/rm -f "$SD/$name.part"
		return 5
	fi
	mv "$SD/$name.part" "$SD/$name" || return 1
	sync
	log "baseos: $name ready; applies on the next restart"
	return 0
}

TRIAL="$SYS/.trial"
TRIES="${AGS_UPDATE_TRIES:-3}"

do_swap() {
	if [ -f "$SD/System.new/slot" ]; then
		/bin/rm -rf "$SD/System.prev"
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
		/bin/rm -rf "$SD/System.failed"
		mv "$SYS" "$SD/System.failed" && mv "$SD/System.prev" "$SYS"
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
