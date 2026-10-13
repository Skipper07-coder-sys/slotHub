//! Where RetroAchievements addresses live in a core's memory, from the maps a
//! core sends with SET_MEMORY_MAPS, as rcheevos's rc_libretro.c finds them.

/// RetroAchievements addresses `ra_start..ra_start + len`, read from `ptr`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RaRegion {
    pub ra_start: u32,
    pub ptr: *const u8,
    pub len: usize,
}

/// One retro_memory_descriptor, kept by value. The pointer is an address so
/// the core that holds these stays Send.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct MapEntry {
    pub ptr: usize,
    pub offset: usize,
    pub start: usize,
    pub select: usize,
    pub disconnect: usize,
    pub len: usize,
}

/// The consoles slot plays, as rcheevos numbers them in rc_consoles.h.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum RaConsole {
    Gb,
    Gba,
    Gbc,
}

impl RaConsole {
    /// rcheevos's RC_CONSOLE_GAMEBOY, _GAMEBOY_ADVANCE and _GAMEBOY_COLOR.
    pub const fn rc_id(self) -> u32 {
        match self {
            RaConsole::Gb => 4,
            RaConsole::Gba => 5,
            RaConsole::Gbc => 6,
        }
    }

    /// The runs of memory rcheevos expects for this console, in its order.
    fn layout(self) -> &'static [ConsoleRegion] {
        match self {
            RaConsole::Gba => &GBA,
            RaConsole::Gb | RaConsole::Gbc => &GB,
        }
    }
}

/// A run of console memory as RetroAchievements numbers it.
struct ConsoleRegion {
    ra_start: u32,
    len: usize,
    real: usize,
    fallback: Fallback,
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum Fallback {
    None,
    SystemRam,
    SaveRam,
}

const fn region(ra_start: u32, end: u32, real: usize) -> ConsoleRegion {
    ConsoleRegion {
        ra_start,
        len: (end - ra_start + 1) as usize,
        real,
        fallback: Fallback::None,
    }
}

/// rcheevos's GBA layout: IWRAM, EWRAM, then cartridge SRAM.
const GBA: [ConsoleRegion; 3] = [
    ConsoleRegion {
        ra_start: 0x00000,
        len: 0x8000,
        real: 0x0300_0000,
        fallback: Fallback::None,
    },
    ConsoleRegion {
        ra_start: 0x08000,
        len: 0x40000,
        real: 0x0200_0000,
        fallback: Fallback::SystemRam,
    },
    ConsoleRegion {
        ra_start: 0x48000,
        len: 0x10000,
        real: 0x0E00_0000,
        fallback: Fallback::SaveRam,
    },
];

/// rcheevos's Game Boy layout, from consoleinfo.c: the CPU's own 64KB as it
/// sees it, then two runs beyond it that hold what the banks hide. Echo RAM
/// reads work RAM again. GB and GBC differ only in that 0x10000-0x15FFF is
/// unused on the GB and work RAM banks 2-7 on the GBC; mGBA's maps put those
/// banks there only on a GBC.
const GB: [ConsoleRegion; 18] = [
    region(0x0000, 0x00FF, 0x0000),    // interrupt vector
    region(0x0100, 0x014F, 0x0100),    // cartridge header
    region(0x0150, 0x3FFF, 0x0150),    // cartridge ROM (fixed)
    region(0x4000, 0x7FFF, 0x4000),    // cartridge ROM (paged)
    region(0x8000, 0x97FF, 0x8000),    // tile RAM
    region(0x9800, 0x9BFF, 0x9800),    // BG1 map data
    region(0x9C00, 0x9FFF, 0x9C00),    // BG2 map data
    region(0xA000, 0xBFFF, 0xA000),    // cartridge RAM (bank 0)
    region(0xC000, 0xCFFF, 0xC000),    // work RAM (bank 0)
    region(0xD000, 0xDFFF, 0xD000),    // work RAM (bank 1)
    region(0xE000, 0xFDFF, 0xC000),    // echo RAM
    region(0xFE00, 0xFE9F, 0xFE00),    // sprite RAM
    region(0xFEA0, 0xFEFF, 0xFEA0),    // unused
    region(0xFF00, 0xFF7F, 0xFF00),    // hardware I/O
    region(0xFF80, 0xFFFE, 0xFF80),    // quick RAM
    region(0xFFFF, 0xFFFF, 0xFFFF),    // interrupt enable
    region(0x10000, 0x15FFF, 0x10000), // GBC work RAM banks 2-7
    region(0x16000, 0x33FFF, 0x16000), // cartridge RAM banks 1-15
];

/// The descriptor holding `real`, and how far into it `real` is.
fn find(maps: &[MapEntry], real: usize) -> Option<(&MapEntry, usize)> {
    for desc in maps {
        if desc.select == 0 {
            if real >= desc.start && real - desc.start < desc.len {
                return Some((desc, real - desc.start));
            }
            continue;
        }
        if (desc.start ^ real) & desc.select != 0 {
            continue;
        }
        let mut reduced = real.wrapping_sub(desc.start);
        // Collapses out the disconnect bits, as RetroArch's mmap_reduce does.
        let mut mask = desc.disconnect;
        while mask != 0 {
            let tmp = (mask - 1) & !mask;
            reduced = (reduced & tmp) | ((reduced >> 1) & !tmp);
            mask = (mask & (mask - 1)) >> 1;
        }
        if reduced < desc.len {
            return Some((desc, reduced));
        }
    }
    None
}

/// A console's RetroAchievements memory, walked region by region through the
/// core's maps as rc_libretro.c does. Where a core maps nothing for a region,
/// the GBA's EWRAM and SRAM come from the plain memory areas instead, as for
/// gpSP, which maps only IWRAM and EWRAM.
pub fn ra_regions(
    console: RaConsole,
    maps: &[MapEntry],
    system_ram: Option<(*const u8, usize)>,
    save_ram: Option<(*const u8, usize)>,
) -> Vec<RaRegion> {
    let mut out = Vec::new();
    for region in console.layout() {
        let mut at = 0usize;
        while at < region.len {
            let real = region.real + at;
            let Some((desc, offset)) = find(maps, real) else {
                break;
            };
            let mut room = desc.len - offset;
            if desc.disconnect != 0 && room > desc.disconnect {
                let low = desc.disconnect & desc.disconnect.wrapping_neg();
                room = low - (real & (low - 1));
            }
            if room == 0 {
                break;
            }
            let n = room.min(region.len - at);
            if desc.ptr != 0 {
                out.push(RaRegion {
                    ra_start: region.ra_start + at as u32,
                    ptr: (desc.ptr + desc.offset + offset) as *const u8,
                    len: n,
                });
            }
            at += n;
        }
        if at > 0 {
            continue;
        }
        let plain = match region.fallback {
            Fallback::None => None,
            Fallback::SystemRam => system_ram,
            Fallback::SaveRam => save_ram,
        };
        if let Some((ptr, len)) = plain.filter(|(p, l)| !p.is_null() && *l > 0) {
            out.push(RaRegion {
                ra_start: region.ra_start,
                ptr,
                len: len.min(region.len),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(buf: &[u8]) -> usize {
        buf.as_ptr() as usize
    }

    #[test]
    fn mgbas_maps_put_iwram_ewram_and_sram_where_rcheevos_reads_them() {
        let iwram = vec![0u8; 0x8000];
        let ewram = vec![0u8; 0x40000];
        let sram = vec![0u8; 0x8000];
        let maps = [
            MapEntry {
                ptr: at(&iwram),
                start: 0x0300_0000,
                select: 0xFF00_0000,
                len: 0x8000,
                ..MapEntry::default()
            },
            MapEntry {
                ptr: at(&ewram),
                start: 0x0200_0000,
                select: 0xFF00_0000,
                len: 0x40000,
                ..MapEntry::default()
            },
            MapEntry {
                ptr: at(&sram),
                start: 0x0E00_0000,
                len: 0x8000,
                ..MapEntry::default()
            },
        ];
        let regions = ra_regions(RaConsole::Gba, &maps, None, None);
        assert_eq!(
            regions,
            [
                RaRegion {
                    ra_start: 0,
                    ptr: iwram.as_ptr(),
                    len: 0x8000
                },
                RaRegion {
                    ra_start: 0x8000,
                    ptr: ewram.as_ptr(),
                    len: 0x40000
                },
                RaRegion {
                    ra_start: 0x48000,
                    ptr: sram.as_ptr(),
                    len: 0x8000
                },
            ]
        );
    }

    #[test]
    fn a_descriptors_offset_and_a_later_start_are_honoured() {
        let ram = vec![0u8; 0x50000];
        let maps = [MapEntry {
            ptr: at(&ram),
            offset: 0x100,
            start: 0x0200_0000,
            len: 0x40000,
            ..MapEntry::default()
        }];
        let regions = ra_regions(RaConsole::Gba, &maps, None, None);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].ra_start, 0x8000);
        assert_eq!(regions[0].ptr as usize, at(&ram) + 0x100);
    }

    #[test]
    fn mirrored_memory_is_read_through_its_disconnect_bits() {
        // 0x8000 bytes mirrored every 0x10000 with select over the top byte.
        let ram = vec![0u8; 0x8000];
        let maps = [MapEntry {
            ptr: at(&ram),
            start: 0x0300_0000,
            select: 0xFF00_0000,
            disconnect: 0x00FF_8000,
            len: 0x8000,
            ..MapEntry::default()
        }];
        let regions = ra_regions(RaConsole::Gba, &maps, None, None);
        assert_eq!(
            regions[0],
            RaRegion {
                ra_start: 0,
                ptr: ram.as_ptr(),
                len: 0x8000
            }
        );
    }

    #[test]
    fn without_maps_ewram_and_sram_come_from_the_plain_memory_areas() {
        let system = vec![0u8; 0x40000];
        let save = vec![0u8; 0x20000];
        let regions = ra_regions(
            RaConsole::Gba,
            &[],
            Some((system.as_ptr(), system.len())),
            Some((save.as_ptr(), save.len())),
        );
        assert_eq!(
            regions,
            [
                RaRegion {
                    ra_start: 0x8000,
                    ptr: system.as_ptr(),
                    len: 0x40000
                },
                RaRegion {
                    ra_start: 0x48000,
                    ptr: save.as_ptr(),
                    len: 0x10000
                },
            ]
        );
    }

    #[test]
    fn gpsps_maps_with_no_sram_fall_back_for_sram_only() {
        let iwram = vec![0u8; 0x8000];
        let ewram = vec![0u8; 0x40000];
        let save = vec![0u8; 0x8000];
        let maps = [
            MapEntry {
                ptr: at(&iwram),
                start: 0x0300_0000,
                len: 0x8000,
                ..MapEntry::default()
            },
            MapEntry {
                ptr: at(&ewram),
                start: 0x0200_0000,
                len: 0x40000,
                ..MapEntry::default()
            },
        ];
        let regions = ra_regions(
            RaConsole::Gba,
            &maps,
            None,
            Some((save.as_ptr(), save.len())),
        );
        assert_eq!(regions.len(), 3);
        assert_eq!(regions[2].ra_start, 0x48000);
        assert_eq!(regions[2].ptr, save.as_ptr());
        assert_eq!(regions[2].len, 0x8000);
    }

    #[test]
    fn a_descriptor_without_a_pointer_leaves_a_gap() {
        let maps = [MapEntry {
            start: 0x0300_0000,
            len: 0x8000,
            ..MapEntry::default()
        }];
        assert!(ra_regions(RaConsole::Gba, &maps, None, None).is_empty());
    }

    /// The maps mGBA's libretro.c sends for a Game Boy game, in its order.
    struct GbMem {
        rom: Vec<u8>,
        vram: Vec<u8>,
        wram: Vec<u8>,
        oam: Vec<u8>,
        io: Vec<u8>,
        hram: Vec<u8>,
        ie: Vec<u8>,
        sram: Vec<u8>,
    }

    impl GbMem {
        fn new(sram: usize) -> GbMem {
            GbMem {
                rom: vec![0; 0x8000],
                vram: vec![0; 0x4000],
                wram: vec![0; 0x8000],
                oam: vec![0; 0xA0],
                io: vec![0; 0x80],
                hram: vec![0; 0x7F],
                ie: vec![0; 1],
                sram: vec![0; sram],
            }
        }

        fn maps(&self, cgb: bool) -> Vec<MapEntry> {
            let e = |buf: &[u8], offset, start, len, select| MapEntry {
                ptr: at(buf),
                offset,
                start,
                select,
                len,
                ..MapEntry::default()
            };
            let mut maps = vec![
                e(&self.rom, 0, 0x0000, 0x4000, 0),
                e(&self.rom, 0x4000, 0x4000, 0x4000, 0),
                e(&self.vram, 0, 0x8000, 0x2000, 0),
                e(&self.wram, 0, 0xC000, 0x1000, 0),
                e(&self.wram, 0x1000, 0xD000, 0x1000, 0),
                e(&self.oam, 0, 0xFE00, 0xA0, 0xFFFF_FF60),
                e(&self.io, 0, 0xFF00, 0x80, 0),
                e(&self.hram, 0, 0xFF80, 0x7F, 0xFFFF_FF80),
                e(&self.ie, 0, 0xFFFF, 1, 0),
            ];
            let save = self.sram.len();
            if save > 0 {
                maps.push(e(&self.sram, 0, 0xA000, save.min(0x2000), 0));
                if save & !0xFF > 0x2000 {
                    maps.push(e(&self.sram, 0x2000, 0x16000, save - 0x2000, 0));
                }
            }
            if cgb {
                maps.push(MapEntry {
                    ptr: at(&self.wram) + 0x2000,
                    start: 0x10000,
                    len: 0x6000,
                    ..MapEntry::default()
                });
            }
            maps
        }
    }

    /// Where RetroAchievements address `ra` reads from, and how far it can
    /// read on from there.
    fn read_at(regions: &[RaRegion], ra: u32) -> Option<(usize, usize)> {
        regions
            .iter()
            .find(|r| ra >= r.ra_start && ((ra - r.ra_start) as usize) < r.len)
            .map(|r| {
                let skip = (ra - r.ra_start) as usize;
                (r.ptr as usize + skip, r.len - skip)
            })
    }

    #[test]
    fn the_console_ids_are_rcheevos_own() {
        assert_eq!(RaConsole::Gb.rc_id(), 4);
        assert_eq!(RaConsole::Gba.rc_id(), 5);
        assert_eq!(RaConsole::Gbc.rc_id(), 6);
    }

    #[test]
    fn the_gb_layouts_follow_consoleinfo_with_no_gaps_or_overlaps() {
        for console in [RaConsole::Gb, RaConsole::Gbc] {
            let layout = console.layout();
            let mut next = 0u32;
            for r in layout {
                assert_eq!(r.ra_start, next, "{console:?}");
                next = r.ra_start + r.len as u32;
            }
            assert_eq!(next, 0x34000, "{console:?}");
        }
    }

    #[test]
    fn mgbas_gbc_maps_put_every_wram_bank_where_rcheevos_reads_it() {
        let mem = GbMem::new(0x8000);
        let regions = ra_regions(RaConsole::Gbc, &mem.maps(true), None, None);
        let wram = at(&mem.wram);
        // Bank 0, bank 1, and banks 2-7 out past the CPU's 64KB.
        assert_eq!(read_at(&regions, 0xC000), Some((wram, 0x1000)));
        assert_eq!(read_at(&regions, 0xD000), Some((wram + 0x1000, 0x1000)));
        assert_eq!(read_at(&regions, 0x10000), Some((wram + 0x2000, 0x6000)));
        assert_eq!(read_at(&regions, 0x15FFF), Some((wram + 0x7FFF, 1)));
        // Echo RAM reads work RAM again.
        assert_eq!(read_at(&regions, 0xE000), Some((wram, 0x1000)));
        assert_eq!(read_at(&regions, 0xF000), Some((wram + 0x1000, 0xE00)));
        // Cartridge RAM bank 0 at 0xA000, banks 1-3 from 0x16000.
        let sram = at(&mem.sram);
        assert_eq!(read_at(&regions, 0xA000), Some((sram, 0x2000)));
        assert_eq!(read_at(&regions, 0x16000), Some((sram + 0x2000, 0x6000)));
        assert_eq!(read_at(&regions, 0x1C000), None);
    }

    #[test]
    fn mgbas_gb_maps_cover_the_cpus_view_of_memory() {
        let mem = GbMem::new(0x2000);
        let regions = ra_regions(RaConsole::Gb, &mem.maps(false), None, None);
        let rom = at(&mem.rom);
        assert_eq!(read_at(&regions, 0x0000), Some((rom, 0x100)));
        assert_eq!(read_at(&regions, 0x0150), Some((rom + 0x150, 0x3EB0)));
        assert_eq!(read_at(&regions, 0x4000), Some((rom + 0x4000, 0x4000)));
        assert_eq!(
            read_at(&regions, 0x9C00),
            Some((at(&mem.vram) + 0x1C00, 0x400))
        );
        assert_eq!(read_at(&regions, 0xA000), Some((at(&mem.sram), 0x2000)));
        assert_eq!(
            read_at(&regions, 0xD000),
            Some((at(&mem.wram) + 0x1000, 0x1000))
        );
        assert_eq!(read_at(&regions, 0xFE00), Some((at(&mem.oam), 0xA0)));
        assert_eq!(read_at(&regions, 0xFEA0), None);
        assert_eq!(read_at(&regions, 0xFF00), Some((at(&mem.io), 0x80)));
        assert_eq!(read_at(&regions, 0xFF80), Some((at(&mem.hram), 0x7F)));
        assert_eq!(read_at(&regions, 0xFFFF), Some((at(&mem.ie), 1)));
        // No GBC banks and no further cartridge RAM on this one.
        assert_eq!(read_at(&regions, 0x10000), None);
        assert_eq!(read_at(&regions, 0x16000), None);
    }

    #[test]
    fn a_gb_game_with_no_cartridge_ram_maps_none() {
        let mem = GbMem::new(0);
        let regions = ra_regions(RaConsole::Gb, &mem.maps(false), None, None);
        assert_eq!(read_at(&regions, 0xA000), None);
        assert!(read_at(&regions, 0xC000).is_some());
    }

    #[test]
    fn the_gb_has_no_fallback_to_the_gba_plain_areas() {
        let system = vec![0u8; 0x8000];
        let save = vec![0u8; 0x2000];
        let regions = ra_regions(
            RaConsole::Gbc,
            &[],
            Some((system.as_ptr(), system.len())),
            Some((save.as_ptr(), save.len())),
        );
        assert!(regions.is_empty());
    }

    #[test]
    fn gpsps_real_maps_read_iwram_past_its_code_tags() {
        // gpSP keeps 0x8000 bytes of tags ahead of IWRAM and says so with
        // the descriptor's offset; it has no SRAM descriptor.
        let iwram = vec![0u8; 0x10000];
        let ewram = vec![0u8; 0x40000];
        let backup = vec![0u8; 0x20000];
        let maps = [
            MapEntry {
                ptr: at(&iwram),
                offset: 0x8000,
                start: 0x0300_0000,
                len: 0x8000,
                ..MapEntry::default()
            },
            MapEntry {
                ptr: at(&ewram),
                start: 0x0200_0000,
                len: 0x40000,
                ..MapEntry::default()
            },
        ];
        let regions = ra_regions(
            RaConsole::Gba,
            &maps,
            Some((ewram.as_ptr(), ewram.len())),
            Some((backup.as_ptr(), backup.len())),
        );
        assert_eq!(read_at(&regions, 0), Some((at(&iwram) + 0x8000, 0x8000)));
        assert_eq!(read_at(&regions, 0x8000), Some((at(&ewram), 0x40000)));
        assert_eq!(read_at(&regions, 0x48000), Some((at(&backup), 0x10000)));
        assert_eq!(read_at(&regions, 0x58000), None);
    }
}
