#!/bin/sh
# Game artwork: fetches a label for every game that has no label yet, from
# the libretro thumbnail server RetroArch uses (thumbnails.libretro.com). The
# game's logo comes first, since slot puts it on the cart's label the way a
# real cartridge shows it; the box is the fallback. Art is named after
# No-Intro game names, so a ROM called
# "Pokemon - Emerald Version (USA, Europe).gba" finds its art; a ROM whose
# name only matches once the bracketed tags are dropped is matched against
# the server's list of names. A GBA ROM that matches neither, a romhack or a
# file renamed by hand, is looked up by the game code in its header (romhacks
# keep their base game's), through libretro-database's serial list, and gets
# that game's art. With a SteamGridDB API key in Config/steamgriddb.conf,
# a game the libretro names miss gets a fan-made logo from SteamGridDB first,
# which is where romhacks have theirs. Labels land in Labels/<GBA|GB|GBC>/<rom>.png,
# where slot looks for them. Verb:
#   fetch   prints "games N" first, "progress K N" as it goes and finally
#           "done FOUND MISSING"
# Exit 0 when it ran, 4 when the server could not be reached, 1 otherwise.
set -u

SD="${SLOT_ROOT:-/mnt/sdcard}"
CURL="${AGS_CURL:-curl}"
SERVER="${AGS_ART_SERVER:-https://thumbnails.libretro.com}"
KINDS="${AGS_ART_KINDS:-Named_Logos Named_Boxarts}"
RUN="${AGS_ART_RUN:-/tmp/slot-art}"
SGDB="${AGS_ART_SGDB:-https://www.steamgriddb.com/api/v2}"
SGDB_KEY_FILE="${AGS_ART_SGDB_KEY:-$SD/Config/steamgriddb.conf}"
SERIALS="${AGS_ART_SERIALS:-https://raw.githubusercontent.com/libretro/libretro-database/master/metadat/serial/Nintendo%20-%20Game%20Boy%20Advance.dat}"
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
	file="$RUN/names-$KIND-$(plain "$1").txt"
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

# The four-letter game code in a GBA header (offset 0xAC), e.g. BPRE.
game_code() {
	code="$(dd if="$1" bs=1 skip=172 count=4 2>/dev/null)"
	case "$code" in
	[A-Z0-9][A-Z0-9][A-Z0-9][A-Z0-9]) echo "$code" ;;
	*) return 1 ;;
	esac
}

# The No-Intro name for a GBA game code, preferring the USA release; the
# serial list is fetched once a run.
name_for_code() {
	file="$RUN/gba-serials.dat"
	if [ ! -s "$file" ]; then
		$CURL -fsSL --connect-timeout 10 --max-time 60 -H 'User-Agent: slot-art' \
			-o "$file" "$SERIALS" 2>/dev/null || { rm -f "$file"; return 1; }
	fi
	awk -v want="AGB-$1-" '
		/^[ \t]*comment "/ { name = $0; sub(/^[ \t]*comment "/, "", name); sub(/"[ \t]*$/, "", name) }
		/^[ \t]*serial "/ && index($0, "\"" want) {
			if (name ~ /\(USA/) { print name; found = 1; exit }
			if (first == "") first = name
		}
		END { if (!found && first != "") print first }' "$file"
}

# The SteamGridDB API key, if the card has one (Config/steamgriddb.conf).
sgdb_key() {
	[ -f "$SGDB_KEY_FILE" ] || return 1
	key="$(sed -e 's/^[^=]*=//' "$SGDB_KEY_FILE" | tr -d ' \t\r\n')"
	[ -n "$key" ] && echo "$key"
}

sgdb_get() {
	$CURL -fsSL -g --connect-timeout 10 --max-time 60 -H 'User-Agent: slot-art' \
		-H "Authorization: Bearer $1" -o "$3" "$2" 2>/dev/null
}

# A fan-made logo from SteamGridDB, found by the game's name without its
# tags and version: what romhacks have, since no retail list knows them.
sgdb_logo() {
	key="$(sgdb_key)" || return 1
	term="$(printf '%s' "$1" | sed -e 's/([^)]*)//g' -e 's/\[[^]]*\]//g' \
		-e 's/[ _-][vV][0-9][0-9.]*.*$//' -e 's/  */ /g' -e 's/^ //' -e 's/ $//')"
	[ -n "$term" ] || return 1
	reply="$RUN/sgdb.json"
	sgdb_get "$key" "$SGDB/search/autocomplete/$(urlencode "$term")" "$reply" || return 1
	# The first hit whose name holds the term (or the other way round):
	# search ranks loosely, and a wrong game's logo is worse than none.
	want="$(plain "$term")"
	id="$(tr ',{}[]' '\n\n\n\n\n' < "$reply" | sed -e 's/\\u00[cCeE]9/e/g' -e 's/\\u[0-9a-fA-F]\{4\}//g' |
		awk -v want="$want" '
			/^"id": *[0-9]/ { id = $0; sub(/^"id": */, "", id) }
			/^"name": *"/ {
				n = $0; sub(/^"name": *"/, "", n); n = tolower(n); gsub(/[^a-z0-9]/, "", n)
				if (id != "" && n != "" && (index(n, want) || index(want, n))) { print id; exit }
				id = ""
			}')"
	[ -n "$id" ] || return 1
	sgdb_get "$key" "$SGDB/logos/game/$id?mimes=image/png" "$reply" || return 1
	url="$(grep -o '"url": *"[^"]*"' "$reply" | head -n 1 | sed -e 's/^"url": *"//' -e 's/"$//' -e 's#\\/#/#g')"
	[ -n "$url" ] || return 1
	get "$url" "$2.part" && mv -f "$2.part" "$2" && log "$1: SteamGridDB logo for $term" && return 0
	rm -f "$2.part"
	return 1
}

# A logo by name, a fan-made logo by name, then by game code, then the same
# for the box. fetch_one's variables are global, so these keep their own.
art_for() {
	code=""
	basegame=""
	looked=""
	for KIND in $KINDS; do
		fetch_one "$1" "$2" "$4" && return 0
		[ "$KIND" = Named_Logos ] && sgdb_logo "$2" "$4" && return 0
		if [ -z "$looked" ] && [ "$1" = GBA ]; then
			looked=1
			code="$(game_code "$3")" && basegame="$(name_for_code "$code")"
		fi
		if [ -n "$basegame" ] && fetch_one "$1" "$basegame" "$4"; then
			log "$2: game code $code, $KIND of $basegame"
			return 0
		fi
	done
	return 1
}

reachable() {
	$CURL -fsS -o /dev/null --connect-timeout 10 --max-time 20 -I "$SERVER/" 2>/dev/null
}

fetch() {
	mkdir -p "$RUN" || return 1
	rm -f "$RUN"/names-*.txt
	if ! reachable; then
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
			printf '%s\t%s\t%s\n' "$dir" "$stem" "$rom" >> "$list"
		done
	done
	total="$(wc -l < "$list" | tr -d ' ')"
	echo "games $total"
	found=0
	missing=0
	n=0
	while IFS="$(printf '\t')" read -r dir stem rom; do
		n=$((n + 1))
		echo "progress $n $total"
		mkdir -p "$SD/Labels/$dir"
		# fetch_one's variables are global, so keep this game's own.
		game="$stem"
		label="$SD/Labels/$dir/$stem.png"
		if art_for "$dir" "$game" "$rom" "$label"; then
			found=$((found + 1))
		elif ! reachable; then
			# Wi-Fi went away mid-run: the rest would all read as Not Found.
			log "lost $SERVER after $found found"
			echo "done $found $missing"
			return 4
		else
			missing=$((missing + 1))
			log "$game: no art found"
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
