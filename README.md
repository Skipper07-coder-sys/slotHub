<div align="center">

<img src=".github/resources/banner.png" width="100%" alt="slot. A bespoke, Game Boy-centric frontend for the Anbernic RG SP.">

<a href="LICENSE"><img src="https://img.shields.io/github/license/BrandonKowalski/slot?style=for-the-badge&labelColor=242429&color=4d4d57" height="36" alt="License"></a>
<a href="https://github.com/BrandonKowalski/slot/stargazers"><img src="https://img.shields.io/github/stars/BrandonKowalski/slot?style=for-the-badge&labelColor=242429&color=4d4d57" height="36" alt="Stars"></a>
<a href="https://github.com/BrandonKowalski/slot/releases"><img src="https://img.shields.io/github/downloads/BrandonKowalski/slot/total?style=for-the-badge&labelColor=242429&color=4d4d57" height="36" alt="Downloads"></a>
<a href="https://slot-cfw.fyi"><img src="https://img.shields.io/badge/Guide-slot--cfw.fyi-4d4d57?style=for-the-badge&labelColor=242429" height="36" alt="Guide"></a>
<a href="CHANGELOG.md"><img src="https://img.shields.io/badge/Changelog-4d4d57?style=for-the-badge&labelColor=242429" height="36" alt="Changelog"></a>

</div>

---

# What is slot?

A bespoke, Game Boy-centric frontend for the Anbernic RG SP.

Has support for GBA, GBC, and GB titles only.

> [!IMPORTANT]
> ROMs must be unzipped. slot does not read `.zip` or `.7z` files.

A full user guide can be found at [slot-cfw.fyi](https://slot-cfw.fyi).

Release notes can be found in the [changelog](CHANGELOG.md).

---

# What does it look like?

<p align="center">
<img src=".github/resources/screenshots/carousel.gif" width="48%" alt="Scrolling the cart carousel across the Game Boy Advance, Game Boy and Game Boy Color shelves">
<img src=".github/resources/screenshots/core-picker.gif" width="48%" alt="Flipping a cart to its circuit board to choose mGBA or gpSP">
</p>

<p align="center">
<img src=".github/resources/screenshots/link.gif" width="96.5%" alt="Two SPs racing in Mario Kart over the emulated link">
</p>

---

# Supported Devices

| Device             | Supported                  | Since Version |
|--------------------|----------------------------|---------------|
| Anbernic RG SP     | Yes                        | Always        |
| Anbernic RG34XX    | Yes                        | 1.4.0         |
| Anbernic RG34XXSP  | Yes (No Stick Support Yet) | 1.4.0         |
| Anbernic RG35XXSP  | Yes                        | 1.5.0         |
| Anbernic 35XX Plus | Yes                        | 1.5.0         |
| Anbernic RG28XX    | Yes                        | 1.6.0         |

RG28XX support was contributed by [GamerAnim](https://github.com/GamerAnim). Its panel is mounted
portrait, so slot turns the picture onto it. If it comes out upside down, add `export SLOT_ROTATE=ccw`
to `System/launch_frontend.sh`.

If you have one of the untested devices, or have tried slot on a device not listed,
please [open an issue on GitHub](https://github.com/BrandonKowalski/slot/issues) and let me know how it went.

---

# Pure Mode

> [!NOTE]
> Pure mode is one of the things this fork (slotHub) adds to slot. It is not part of upstream slot,
> and both the code and this section were written with Claude.

For when you would rather not have the option. Pure mode runs a game the way the handheld
would have, and keeps everything that only changes how it looks and sounds.

To switch it on, create `Config/pure.txt` on the card that holds slot, with one line in it:

```
pure = on
```

There is no setting for it on the device. Switching it off means putting the card in a
computer and changing the line to `pure = off`, or deleting the file.

| With pure on                                     |                                                    |
|--------------------------------------------------|----------------------------------------------------|
| Fast-forward on `R2`                             | Off                                                |
| Rewind on `L2`                                   | Off                                                |
| Turbo `A` and `B` on `X` and `Y`                 | Off                                                |
| Save states on `SELECT` + `R1` / `L1`            | Off. `SELECT`, `L` and `R` reach the game instead  |
| Save state switcher on a double tap of `MENU`    | Off                                                |
| Resuming where you left off                      | Once, see below                                    |
| In-game saves                                    | Unchanged                                          |
| Shaders, palettes, color correction              | Unchanged                                          |
| Brightness, blue light, volume, rumble, linking  | Unchanged                                          |

The Gameplay page of the settings shrinks to Rumble and Auto Save on Eject.

## Resuming, once

Closing the lid, powering off and ejecting a cart still pick up where you left off. What
they leave on the card is deleted as soon as that game is running again, so the same moment
can never be loaded a second time.

- Holding `A` to start a cart fresh throws the suspended game away.
- A reset or a battery that dies in the middle of a game goes back to your last in-game
  save, as it would have on a cartridge. While you play, the only thing kept from the game
  is its own in-game save.
- Turn Auto Save on Eject off and an eject is a cart pulled out of a running console.

Save states made before pure was switched on are left alone on the card. They are there
again the day you switch it off, along with your fast-forward, rewind and turbo settings.

---

# Bluetooth Headphones

> [!NOTE]
> Bluetooth is a slotHub addition, not part of upstream slot, written with Claude. It needs
> BaseOS on a device with Bluetooth (the RG34XXSP has it), which ships the driver, BlueZ and BlueALSA.

Open the settings with `MENU` and choose Bluetooth.

- **Bluetooth** switches the radio on or off. It stays as you left it across restarts, and
  when it is on slot reconnects the headphones you chose last.
- **Headphones** shows whether a pair is connected, and its name. Press `A` to search:
  slot listens for about 12 seconds and lists every pair of headphones or speaker it hears,
  nearest first. Put yours in pairing mode first (for AirPods, hold the case button until
  the light flashes white), pick them and press `A`. slot pairs, trusts and connects them,
  and remembers them in `bluetooth.conf` at the root of the card, so they reconnect by
  themselves whenever Bluetooth is on. `X` searches again.

While headphones are connected the game's sound goes to them instead of the speaker, and
slot keeps the stream open even in silence, since this BlueALSA drops it otherwise. The
radio and BlueALSA are driven by `System/bluetooth.sh`; it logs everything it does, and a
dump of what BlueZ knows after every start and failed connect, to `slot-bluetooth.log` at
the root of the card.

# Wi-Fi

slot can join your home Wi-Fi so you can reach the card over the network. Open the quick menu,
go to **Wi-Fi** and pick **Choose Network**. slot lists the networks in range, strongest first.
Pick yours, type its password on the on-screen keyboard and press **Start** (or **Join**):

| Button | On the keyboard |
|--------|-----------------|
| A      | Type the key under the cursor |
| B      | Delete the last character, or go back to the list when there is none |
| X      | Switch between lower case, upper case and symbols |
| Y      | Space |
| Start  | Join |

slot saves each network you join to `wifi.conf` at the root of the card, newest first, and joins
whichever saved network is in range every time Wi-Fi is on. You can also edit that file on a
computer:

```ini
# The first network listed is preferred when both are in range
ssid=Home
psk=my wifi password

ssid=Phone Hotspot
psk=another password
```

Leave the `psk` line out for an open network. **Wi-Fi** switches it on or off, and the **Network**
row shows how joining went:

| Network shows | What it means |
|---------------|---------------|
| Joining...    | Looking for a listed network and getting an address |
| Connected     | Online. The handheld answers at `<hostname>.local`, eg `rg34xxsp.local` |
| None Saved    | No network has been saved yet, or none has a 8 to 63 character password |
| Not Found     | None of the listed networks is in range, or the password is wrong |
| No Address    | Joined, but the router gave no address |
| Failed        | The radio did not start. Details are in `slot-wifi.log` |

Press **A** on **Network** to try again. Wi-Fi stays on across restarts until you switch it off.
Link play takes over the radio while a link is up and hands it back afterwards.

Once connected, BaseOS's SSH server lets you copy games, labels and saves with any SFTP app
(Cyberduck, ForkLift, WinSCP or `scp`): log in as `root` at `rg34xxsp.local`. **Set your own SSH
password first**: BaseOS uses `root` until you add `ssh_password=` to `baseos.conf`.

# Updates

With Wi-Fi connected, slot can fetch its own updates and BaseOS's. Open the quick menu and go to
**Update**. The page checks GitHub and shows one row for each:

| Row shows | What it means |
|-----------|---------------|
| Turn on Wi-Fi | Updates need a connection; switch Wi-Fi on first |
| Checking... | Asking GitHub for the latest releases |
| Get v1.8.0 | A newer release is out. Press **A** to download it |
| v1.7.0 Up to Date | The installed release is the newest |
| Downloading... | Fetching and checking the download |
| Restart to Install | The download is on the card. Press **A** to restart and install it |
| No Connection | GitHub could not be reached. Press **A** to check again |
| Failed, Retry | The download did not complete or did not match its checksum. Press **A** to check again |

**slot** downloads the release zip from this repository, checks it against the release's
`SHA256SUMS`, and unpacks it next to `System` as `System.new`. The restart swaps it in and keeps
the old one as `System.prev`. If the new slot fails to come up three starts in a row, the old one
is put back on its own (the broken one is left as `System.failed` for a look). Once the new slot
has run for a few seconds, `System.prev` is removed. Your games, saves and label cache stay where
they are.

**BaseOS** downloads the `.bosupd` payload for your device to the card root. BaseOS installs it on
the next restart, into its spare system slot, and rolls back by itself if the new system does not
boot.

Pre-releases are opt-in: put a file named `update-channel` containing `beta` at the card root and
the page offers the newest release of any kind. `slot-update.log` at the card root says what each
download did.

# AI Disclosure

The Rust frontend was put together by Claude Opus. I reviewed everything that was
produced. All documentation is 100% free-range, meatbag prose.

The project is extremely low stakes. I wanted a bespoke frontend for my RG SP and thought
that something that evokes the feeling of using my GBA SP as a kid would be pretty neat.

Use it, don't use it, I don't care.

Figured I should share the end result of all the wasted water. ✌🏻
