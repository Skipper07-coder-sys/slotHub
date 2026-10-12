#!/bin/sh
set -eu

HERE="$(cd "$(dirname "$0")" && pwd)"
SCRIPT="$HERE/System/art.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT HUP INT TERM
FAILS=0

fail() {
	echo "FAIL: $*"
	FAILS=$((FAILS + 1))
}

# A stand-in for the thumbnail server: files under $TMP/server, a directory
# answering with an index page that links each PNG, URL-encoded.
mkdir -p "$TMP/bin"
cat > "$TMP/bin/curl" <<FAKE
#!/bin/sh
printf '%s\n' "\$*" >> "$TMP/curl.log"
[ -n "\${FAKE_OFFLINE:-}" ] && exit 7
out=""
url=""
head=""
while [ \$# -gt 0 ]; do
	case "\$1" in
	-o) out="\$2"; shift ;;
	-H|--connect-timeout|--max-time) shift ;;
	-I) head=1 ;;
	http*) url="\$1" ;;
	esac
	shift
done
[ -n "\$head" ] && exit 0
path="\$(printf '%s' "\${url#http://art}" | sed -e 's/%20/ /g' -e 's/%28/(/g' -e 's/%29/)/g' -e 's/%2C/,/g' -e 's/%27/'"'"'/g' -e 's/%23/#/g' -e 's/%2B/+/g' -e 's/%5B/[/g' -e 's/%5D/]/g' -e 's/%25/%/g')"
file="$TMP/server\$path"
case "\$path" in
*/)
	[ -d "\$file" ] || exit 22
	for f in "\$file"*.png; do
		[ -e "\$f" ] || continue
		n="\$(basename "\$f" | sed -e 's/ /%20/g' -e 's/(/%28/g' -e 's/)/%29/g' -e 's/,/%2C/g')"
		echo "<a href=\"\$n\">x</a>"
	done
	;;
*)
	[ -f "\$file" ] || exit 22
	cp "\$file" "\$out"
	;;
esac
FAKE
chmod 755 "$TMP/bin/curl"

art() {
	mkdir -p "$TMP/server/Nintendo - Game Boy Advance/Named_Boxarts" "$TMP/server/Nintendo - Game Boy/Named_Boxarts" "$TMP/server/Nintendo - Game Boy Color/Named_Boxarts"
	echo "$2" > "$TMP/server/Nintendo - $1/Named_Boxarts/$2.png"
}

rom() {
	mkdir -p "$TMP/card/Games/$1"
	: > "$TMP/card/Games/$1/$2"
}

run() {
	SLOT_ROOT="$TMP/card" AGS_CURL="$TMP/bin/curl" AGS_ART_SERVER="http://art" AGS_ART_RUN="$TMP/run" \
		sh "$SCRIPT" "$@"
}

art "Game Boy Advance" "Pokemon - Emerald Version (USA, Europe)"
art "Game Boy Advance" "Metroid Fusion (Europe) (En,Fr,De,Es,It)"
art "Game Boy Advance" "Metroid Fusion (USA)"
art "Game Boy Color" "Pokemon - Crystal Version (USA, Europe) (Rev 1)"
art "Game Boy Advance" "Mario & Luigi - Superstar Saga (USA)"
mkdir -p "$TMP/server/Nintendo - Game Boy Advance/Named_Boxarts"
echo amp > "$TMP/server/Nintendo - Game Boy Advance/Named_Boxarts/Mario _ Luigi - Superstar Saga (USA).png"

rom GBA "Pokemon - Emerald Version (USA, Europe).gba"
rom GBA "Metroid Fusion.gba"
rom GBA "Mario & Luigi - Superstar Saga (USA).GBA"
rom GBA "Homebrew Thing.gba"
rom GBA "notes.txt"
rom GB "Pokemon - Crystal Version (USA, Europe) (Rev 1).gbc"
mkdir -p "$TMP/card/Labels/GBA"
echo mine > "$TMP/card/Labels/GBA/homebrew thing.png"

out="$(run fetch)" && rc=0 || rc=$?
[ "$rc" = 0 ] || fail "fetch exited $rc, want 0"
echo "$out" | grep -qx "games 4" || fail "want 4 games without labels, got: $out"
echo "$out" | grep -qx "progress 4 4" || fail "no progress lines: $out"
echo "$out" | tail -n 1 | grep -qx "done 4 0" || fail "want done 4 0, got: $(echo "$out" | tail -n 1)"
L="$TMP/card/Labels"
grep -qx "Pokemon - Emerald Version (USA, Europe)" "$L/GBA/Pokemon - Emerald Version (USA, Europe).png" 2>/dev/null || fail "exact name not fetched"
grep -qx "Metroid Fusion (USA)" "$L/GBA/Metroid Fusion.png" 2>/dev/null || fail "loose match did not prefer the USA box"
grep -qx "amp" "$L/GBA/Mario & Luigi - Superstar Saga (USA).png" 2>/dev/null || fail "& was not swapped for _"
[ -f "$L/GB/Pokemon - Crystal Version (USA, Europe) (Rev 1).png" ] || fail "a GBC game in GB/ was not looked up as Game Boy Color"
grep -qx mine "$L/GBA/homebrew thing.png" || fail "a label already there was replaced"
ls "$L"/*/*.part >/dev/null 2>&1 && fail "a partial download was left behind"

rm -f "$L/GBA/Metroid Fusion.png"
rm -f "$TMP/server/Nintendo - Game Boy Advance/Named_Boxarts/Metroid Fusion"*
out="$(run fetch)" || true
echo "$out" | tail -n 1 | grep -qx "done 0 1" || fail "a game with no art: want done 0 1, got: $out"

# A game's logo is preferred to its box.
mkdir -p "$TMP/server/Nintendo - Game Boy Advance/Named_Logos"
echo logo > "$TMP/server/Nintendo - Game Boy Advance/Named_Logos/Metroid Fusion (USA).png"
echo box > "$TMP/server/Nintendo - Game Boy Advance/Named_Boxarts/Metroid Fusion (USA).png"
out="$(run fetch)" || true
grep -qx logo "$L/GBA/Metroid Fusion.png" 2>/dev/null || fail "the box was taken over the logo"

# A romhack keeps its base game's code at 0xAC and gets that game's logo.
echo fr-logo > "$TMP/server/Nintendo - Game Boy Advance/Named_Logos/Pokemon - FireRed Version (USA).png"
cat > "$TMP/server/serials.dat" <<'DAT'
game (
	comment "Pokemon - FireRed Version (Europe)"
	serial "AGB-BPRP-EUR"
)
game (
	comment "Pokemon - FireRed Version (USA)"
	serial "AGB-BPRE-USA"
)
DAT
{ head -c 172 /dev/zero; printf BPRE; head -c 20 /dev/zero; } > "$TMP/card/Games/GBA/Radical Red v4.1.gba"
out="$(SLOT_ROOT="$TMP/card" AGS_CURL="$TMP/bin/curl" AGS_ART_SERVER="http://art" AGS_ART_RUN="$TMP/run" \
	AGS_ART_SERIALS="http://art/serials.dat" sh "$SCRIPT" fetch)" || true
grep -qx fr-logo "$L/GBA/Radical Red v4.1.png" 2>/dev/null || fail "romhack did not get its base game's logo: $out"

# With a SteamGridDB key, a hack the libretro names miss gets its fan-made
# logo before its base game's.
mkdir -p "$TMP/card/Config" "$TMP/server/sgdb/search/autocomplete" "$TMP/server/sgdb/logos/game/4242?mimes=image"
echo "key = abc123" > "$TMP/card/Config/steamgriddb.conf"
echo '{"success":true,"data":[{"types":["steam"],"id":7,"name":"Pok\u00e9mon Pinball"},{"types":[],"id":4242,"name":"Pok\u00e9mon Unbound"}]}' \
	> "$TMP/server/sgdb/search/autocomplete/Pokemon Unbound"
echo '{"success":true,"data":[{"id":1,"score":3,"url":"http:\/\/art\/cdn\/unbound.png"}]}' \
	> "$TMP/server/sgdb/logos/game/4242?mimes=image/png"
mkdir -p "$TMP/server/cdn"
echo fan-logo > "$TMP/server/cdn/unbound.png"
{ head -c 172 /dev/zero; printf BPRE; head -c 20 /dev/zero; } > "$TMP/card/Games/GBA/Pokemon Unbound (v2.1.1.1).gba"
out="$(SLOT_ROOT="$TMP/card" AGS_CURL="$TMP/bin/curl" AGS_ART_SERVER="http://art" AGS_ART_RUN="$TMP/run" \
	AGS_ART_SERIALS="http://art/serials.dat" AGS_ART_SGDB="http://art/sgdb" sh "$SCRIPT" fetch)" || true
grep -qx fan-logo "$L/GBA/Pokemon Unbound (v2.1.1.1).png" 2>/dev/null \
	|| fail "hack did not get its SteamGridDB logo: $out $(cat "$TMP/run/art.log" 2>/dev/null)"
grep -q "Authorization: Bearer abc123" "$TMP/curl.log" || fail "the key was not sent"

# A bare name that would find the wrong games is searched as a Pokemon hack
# when its header says POKEMON, and every word must be in the hit's name.
echo '{"success":true,"data":[{"id":9,"name":"Extended Stay"}]}' > "$TMP/server/sgdb/search/autocomplete/extended"
echo '{"success":true,"data":[{"id":55,"name":"Pok\u00e9mon Fire Red Extended"}]}' \
	> "$TMP/server/sgdb/search/autocomplete/Pokemon extended"
mkdir -p "$TMP/server/sgdb/logos/game/55?mimes=image"
echo '{"data":[{"url":"http:\/\/art\/cdn\/ext.png","thumb":"x"}]}' > "$TMP/server/sgdb/logos/game/55?mimes=image/png"
echo ext-logo > "$TMP/server/cdn/ext.png"
{ head -c 160 /dev/zero; printf 'POKEMON FIRE'; printf BPRE; head -c 20 /dev/zero; } > "$TMP/card/Games/GBA/extended v3.5.6.gba"
out="$(SLOT_ROOT="$TMP/card" AGS_CURL="$TMP/bin/curl" AGS_ART_SERVER="http://art" AGS_ART_RUN="$TMP/run" \
	AGS_ART_SERIALS="http://art/serials.dat" AGS_ART_SGDB="http://art/sgdb" sh "$SCRIPT" fetch)" || true
grep -qx ext-logo "$L/GBA/extended v3.5.6.png" 2>/dev/null || fail "Pokemon retry did not find the hack: $out"

FAKE_OFFLINE=1 run fetch > /dev/null && rc=0 || rc=$?
[ "$rc" = 4 ] || fail "offline fetch exited $rc, want 4"

if [ "$FAILS" -gt 0 ]; then
	echo "$FAILS failure(s)"
	exit 1
fi
echo "art.sh: all passed"
