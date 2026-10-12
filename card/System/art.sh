#!/bin/sh
# Game artwork: fetches box art for every game that has no label yet, from
# the libretro thumbnail server RetroArch uses (thumbnails.libretro.com). Art
# is named after No-Intro game names, so a ROM called
# "Pokemon - Emerald Version (USA, Europe).gba" finds its box; a ROM whose
# name only matches once the bracketed tags are dropped is matched against
# the server's list of names. Labels land in Labels/<GBA|GB|GBC>/<rom>.png,
# where slot looks for them. Verb:
#   fetch   prints "games N" first, "progress K N" as it goes and finally
#           "done FOUND MISSING"
# Exit 0 when it ran, 4 when the server could not be reached, 1 otherwise.
set -u

SD="${SLOT_ROOT:-/mnt/sdcard}"
CURL="${AGS_CURL:-curl}"
SERVER="${AGS_ART_SERVER:-https://thumbnails.libretro.com}"
KIND="${AGS_ART_KIND:-Named_Boxarts}"
RUN="${AGS_ART_RUN:-/tmp/slot-art}"
LOG="$RUN/art.log"

log() {
	echo "$(date '+%H:%M:%S') $*" >> "$LOG" 2>/dev/null
	return 0
}

systems() {
	case "$1" in
	GBA) echo "Nintendo - Game Boy Advance" ;;
	GB) printf '%s\n%s\n' "Nintendo - Game Boy" "Nintendo - Game Boy Color" ;;
	GBC) printf '%s\n%s\n' "Nintendo - Game Boy Color" "Nintendo - Game Boy" ;;
	esac
}

is_rom() {
	case "$1" in
	GBA) case "$2" in *.[gG][bB][aA]) return 0 ;; esac ;;
	*) case "$2" in *.[gG][bB] | *.[gG][bB][cC]) return 0 ;; esac ;;
	esac
	return 1
}

# Thumbnail names swap the characters a file name can't hold for "_".
art_name() {
	printf '%s' "$1" | sed 's/[&*\/:`<>?\\|"]/_/g'
}

urlencode() {
	printf '%s' "$1" | sed -e 's/%/%25/g' -e 's/ /%20/g' -e 's/#/%23/g' -e 's/+/%2B/g' \
		-e 's/\[/%5B/g' -e 's/\]/%5D/g' -e "s/'/%27/g"
}

# For loose matching: lower case, no bracketed tags, letters and digits only.
plain() {
	printf '%s' "$1" | sed -e 's/([^)]*)//g' -e 's/\[[^]]*\]//g' | tr 'A-Z' 'a-z' | tr -cd 'a-z0-9'
}

get() {
	$CURL -fsSL -g --connect-timeout 10 --max-time 60 -H 'User-Agent: slot-art' -o "$2" "$1" 2>/dev/null
}

# The server's names for one system, fetched once a run, each after its plain
# form: the directory page links every PNG.
names_of() {
	file="$RUN/names-$(plain "$1").txt"
	if [ ! -s "$file" ]; then
		$CURL -fsSL -g --connect-timeout 10 --max-time 60 -H 'User-Agent: slot-art' \
			"$SERVER/$(urlencode "$1")/$KIND/" 2>/dev/null |
			sed -n 's/.*href="\([^"]*\.png\)".*/\1/p' |
			sed -e 's/%20/ /g' -e 's/%28/(/g' -e 's/%29/)/g' -e 's/%2C/,/g' -e "s/%27/'/g" \
				-e 's/%26/\&/g' -e 's/%21/!/g' -e 's/%2B/+/g' -e 's/%5B/[/g' -e 's/%5D/]/g' \
				-e 's/%25/%/g' -e 's/\.png$//' |
			awk '{ p = $0; gsub(/\([^)]*\)/, "", p); gsub(/\[[^]]*\]/, "", p)
				p = tolower(p); gsub(/[^a-z0-9]/, "", p); print p "\t" $0 }' > "$file"
	fi
	cat "$file"
}

# Picks the server's name whose plain form matches, preferring a USA release.
loose() {
	want="$(plain "$2")"
	[ -n "$want" ] || return 1
	names_of "$1" | awk -F '\t' -v want="$want" '
		$1 == want && $2 ~ /\(USA/ { print $2; found = 1; exit }
		$1 == want && first == "" { first = $2 }
		END { if (!found && first != "") print first }'
}

fetch_one() {
	dir="$1"
	stem="$2"
	out="$3"
	tmp="$out.part"
	systems "$dir" > "$RUN/systems.txt"
	while IFS= read -r system; do
		base="$SERVER/$(urlencode "$system")/$KIND"
		if get "$base/$(urlencode "$(art_name "$stem")").png" "$tmp"; then
			mv -f "$tmp" "$out" && return 0
		fi
		name="$(loose "$system" "$stem")"
		if [ -n "$name" ] && get "$base/$(urlencode "$(art_name "$name")").png" "$tmp"; then
			log "$stem: matched $name"
			mv -f "$tmp" "$out" && return 0
		fi
	done < "$RUN/systems.txt"
	rm -f "$tmp"
	return 1
}

fetch() {
	mkdir -p "$RUN" || return 1
	rm -f "$RUN"/names-*.txt
	if ! $CURL -fsS -o /dev/null --connect-timeout 10 --max-time 20 -I "$SERVER/" 2>/dev/null; then
		log "no answer from $SERVER"
		return 4
	fi
	list="$RUN/todo.txt"
	: > "$list"
	for dir in GBA GB GBC; do
		[ -d "$SD/Games/$dir" ] || continue
		for rom in "$SD/Games/$dir"/*; do
			[ -f "$rom" ] || continue
			file="${rom##*/}"
			case "$file" in .*) continue ;; esac
			is_rom "$dir" "$file" || continue
			stem="${file%.*}"
			have="$(ls "$SD/Labels/$dir" 2>/dev/null | grep -i -x -F "$stem.png")"
			[ -n "$have" ] && continue
			printf '%s\t%s\n' "$dir" "$stem" >> "$list"
		done
	done
	total="$(wc -l < "$list" | tr -d ' ')"
	echo "games $total"
	found=0
	missing=0
	n=0
	while IFS="$(printf '\t')" read -r dir stem; do
		n=$((n + 1))
		echo "progress $n $total"
		mkdir -p "$SD/Labels/$dir"
		if fetch_one "$dir" "$stem" "$SD/Labels/$dir/$stem.png"; then
			found=$((found + 1))
		else
			missing=$((missing + 1))
			log "$stem: no art found"
		fi
	done < "$list"
	log "found $found, missing $missing"
	echo "done $found $missing"
}

case "${1:-}" in
fetch) fetch ;;
*)
	echo "usage: $0 fetch" >&2
	exit 1
	;;
esac
