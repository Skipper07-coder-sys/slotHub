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
> Pure mode is the one thing this fork (slotHub) adds to slot. It is not part of upstream slot,
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

# AI Disclosure

The Rust frontend was put together by Claude Opus. I reviewed everything that was
produced. All documentation is 100% free-range, meatbag prose.

The project is extremely low stakes. I wanted a bespoke frontend for my RG SP and thought
that something that evokes the feeling of using my GBA SP as a kid would be pretty neat.

Use it, don't use it, I don't care.

Figured I should share the end result of all the wasted water. ✌🏻
