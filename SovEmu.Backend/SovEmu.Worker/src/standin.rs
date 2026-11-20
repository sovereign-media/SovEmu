//! Stand-in games for the probe.
//!
//! Some cores declare their options only once a game is loaded (FCEUmm and Dolphin in libretro's
//! stable release), so a probe without a game sees none. For those, the probe loads the
//! smallest valid program the core accepts, chosen by the core's file extensions rather than its
//! name, reads the options, and unloads it. Nothing is run: loading is enough.
//!
//! Arcade cores have no such stand-in (they want a real ROM set); their options come from the
//! first game session instead (`Reply::Options`).

/// A stand-in for a core accepting `extensions`: its extension and bytes.
pub(crate) fn for_extensions(extensions: &[String]) -> Option<(&'static str, Vec<u8>)> {
    let accepts = |ext: &str| extensions.iter().any(|e| e.eq_ignore_ascii_case(ext));
    if accepts("nes") {
        Some(("nes", nes()))
    } else if accepts("dol") {
        Some(("dol", dol()))
    } else {
        None
    }
}

/// An iNES cartridge (mapper 0, one 16 KiB PRG bank, one 8 KiB CHR bank) whose program is
/// `JMP $8000`, with every vector pointing at it.
fn nes() -> Vec<u8> {
    const PRG: usize = 16 * 1024;
    const CHR: usize = 8 * 1024;
    let mut rom = Vec::with_capacity(16 + PRG + CHR);
    rom.extend_from_slice(b"NES\x1a");
    rom.extend_from_slice(&[1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let mut prg = vec![0u8; PRG];
    prg[..3].copy_from_slice(&[0x4C, 0x00, 0x80]); // JMP $8000
    // NMI, RESET and IRQ vectors at $FFFA-$FFFF (the bank is mirrored at $C000).
    for v in prg[PRG - 6..].chunks_mut(2) {
        v.copy_from_slice(&[0x00, 0x80]);
    }
    rom.extend_from_slice(&prg);
    rom.extend(std::iter::repeat_n(0u8, CHR));
    rom
}

/// A GameCube DOL executable: one text section at 0x80003100 holding `b .` (branch to itself),
/// which is also the entry point.
fn dol() -> Vec<u8> {
    const TEXT_OFFSET: u32 = 0x100;
    const LOAD: u32 = 0x8000_3100;
    const CODE: [u8; 4] = [0x48, 0x00, 0x00, 0x00]; // b .
    let mut dol = vec![0u8; TEXT_OFFSET as usize];
    let put =
        |dol: &mut Vec<u8>, at: usize, v: u32| dol[at..at + 4].copy_from_slice(&v.to_be_bytes());
    put(&mut dol, 0x00, TEXT_OFFSET); // text section 0: file offset
    put(&mut dol, 0x48, LOAD); // its load address
    put(&mut dol, 0x90, 0x20); // its size (32 bytes)
    put(&mut dol, 0xE0, LOAD); // entry point
    for _ in 0..8 {
        dol.extend_from_slice(&CODE);
    }
    dol
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_by_extension() {
        let exts = |l: &[&str]| l.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(for_extensions(&exts(&["fds", "NES", "unf"])).unwrap().0, "nes");
        assert_eq!(for_extensions(&exts(&["iso", "gcm", "dol"])).unwrap().0, "dol");
        assert!(for_extensions(&exts(&["zip", "7z"])).is_none(), "no stand-in for arcade sets");
    }

    #[test]
    fn the_cartridge_is_a_valid_ines_image() {
        let rom = nes();
        assert_eq!(rom.len(), 16 + 16 * 1024 + 8 * 1024);
        assert_eq!(&rom[..4], b"NES\x1a");
        assert_eq!((rom[4], rom[5]), (1, 1), "one PRG and one CHR bank");
        assert_eq!(&rom[16..19], &[0x4C, 0x00, 0x80]);
        let reset = 16 + 16 * 1024 - 4;
        assert_eq!(&rom[reset..reset + 2], &[0x00, 0x80], "RESET points at the program");
    }

    #[test]
    fn the_program_is_a_valid_dol() {
        let dol = dol();
        let word = |at: usize| u32::from_be_bytes(dol[at..at + 4].try_into().unwrap());
        assert_eq!(dol.len(), 0x100 + 0x20);
        assert_eq!((word(0x00), word(0x48), word(0x90)), (0x100, 0x8000_3100, 0x20));
        assert_eq!(word(0xE0), 0x8000_3100, "entry is the text section");
        assert_eq!(word(0x100), 0x4800_0000);
    }
}
