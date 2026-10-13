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

/// The GBA's RetroAchievements memory, from the core's maps where it sent
/// them and from its plain memory areas where it did not.
pub fn gba_regions(
    maps: &[MapEntry],
    system_ram: Option<(*const u8, usize)>,
    save_ram: Option<(*const u8, usize)>,
) -> Vec<RaRegion> {
    let mut out = Vec::new();
    for region in &GBA {
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
        let regions = gba_regions(&maps, None, None);
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
        let regions = gba_regions(&maps, None, None);
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
        let regions = gba_regions(&maps, None, None);
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
        let regions = gba_regions(
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
        let regions = gba_regions(&maps, None, Some((save.as_ptr(), save.len())));
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
        assert!(gba_regions(&maps, None, None).is_empty());
    }
}
