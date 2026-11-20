//! Soft patching: a ROM is patched as it is loaded (a translation, a fix, a hack), never on
//! disk; the library stays as it is. The three formats RetroArch and Recalbox apply:
//!
//! - **IPS**: records of `offset, bytes` (or a run of one byte), optionally truncating at the end.
//!   No checksums: a patch for another dump of the game applies anyway, wrongly.
//! - **UPS**: the target as the source XORed with the patch's bytes; CRC32s of the source, the
//!   target and the patch.
//! - **BPS**: the target built from copies of the source, of the patch and of itself; the same
//!   three CRC32s. Most current translations ship as BPS.
//!
//! A UPS or BPS patch for another version of the ROM is refused (its source checksum differs),
//! which is why they are preferred.

use std::fmt;
use std::path::{Path, PathBuf};

/// Why a patch could not be applied.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PatchError {
    /// Not an IPS, UPS or BPS file.
    UnknownFormat,
    /// The patch ends early or has an invalid record.
    Corrupt(&'static str),
    /// The patch's own checksum is wrong: it is damaged.
    PatchChecksum,
    /// The patch is for another version of the game (its source checksum differs).
    WrongSource,
    /// Applying it produced something other than what the patch expects.
    TargetChecksum,
}

impl fmt::Display for PatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownFormat => write!(f, "not an IPS, UPS or BPS patch"),
            Self::Corrupt(what) => write!(f, "the patch is damaged ({what})"),
            Self::PatchChecksum => write!(f, "the patch is damaged (its checksum is wrong)"),
            Self::WrongSource => {
                write!(f, "the patch is for another version of this game (checksum mismatch)")
            }
            Self::TargetChecksum => write!(f, "the patched game is not what the patch expects"),
        }
    }
}

impl std::error::Error for PatchError {}

/// Applies `patch` (any of the three formats, told apart by its header) to `source`.
pub(crate) fn apply(source: &[u8], patch: &[u8]) -> Result<Vec<u8>, PatchError> {
    if patch.starts_with(b"PATCH") {
        ips(source, patch)
    } else if patch.starts_with(b"UPS1") {
        ups(source, patch)
    } else if patch.starts_with(b"BPS1") {
        bps(source, patch)
    } else {
        Err(PatchError::UnknownFormat)
    }
}

/// A patched copy of a ROM in a temporary folder, removed when dropped. It keeps the ROM's
/// extension (cores go by it) and is named `<rom stem> [<patch name>]`, so its battery save is
/// the patched game's own.
pub(crate) struct PatchedRom {
    dir: PathBuf,
    pub path: PathBuf,
}

impl PatchedRom {
    pub(crate) fn write(rom: &Path, patch: &Path, name: &str) -> anyhow::Result<Self> {
        use anyhow::Context;
        let source = std::fs::read(rom).with_context(|| format!("reading {}", rom.display()))?;
        let bytes = std::fs::read(patch).with_context(|| format!("reading {}", patch.display()))?;
        let patched =
            apply(&source, &bytes).with_context(|| format!("applying {}", patch.display()))?;
        let dir = std::env::temp_dir().join(format!("sovemu-patched-{}", std::process::id()));
        std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        let stem = rom.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let name = name.replace(['/', '\\'], "-");
        let mut file = format!("{stem} [{name}]");
        if let Some(ext) = rom.extension() {
            file.push('.');
            file.push_str(&ext.to_string_lossy());
        }
        let path = dir.join(file);
        std::fs::write(&path, patched).with_context(|| format!("writing {}", path.display()))?;
        tracing::info!(patch = %patch.display(), "patched the game");
        Ok(Self { dir, path })
    }
}

impl Drop for PatchedRom {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn ips(source: &[u8], patch: &[u8]) -> Result<Vec<u8>, PatchError> {
    let mut out = source.to_vec();
    let mut at = 5;
    let byte = |i: usize| patch.get(i).copied().ok_or(PatchError::Corrupt("ends early"));
    loop {
        if patch.get(at..at + 3) == Some(b"EOF") {
            at += 3;
            // An optional 3-byte length the target is truncated to.
            if let Some(len) = patch.get(at..at + 3) {
                let len =
                    (usize::from(len[0]) << 16) | (usize::from(len[1]) << 8) | usize::from(len[2]);
                out.truncate(len);
            }
            return Ok(out);
        }
        let offset = (usize::from(byte(at)?) << 16)
            | (usize::from(byte(at + 1)?) << 8)
            | usize::from(byte(at + 2)?);
        let size = (usize::from(byte(at + 3)?) << 8) | usize::from(byte(at + 4)?);
        at += 5;
        let (run, value_at) = if size == 0 {
            // A run: a 2-byte count of one byte.
            let count = (usize::from(byte(at)?) << 8) | usize::from(byte(at + 1)?);
            at += 3;
            (count, None)
        } else {
            at += size;
            (size, Some(at - size))
        };
        if out.len() < offset + run {
            out.resize(offset + run, 0);
        }
        match value_at {
            Some(from) => {
                let data = patch.get(from..from + run).ok_or(PatchError::Corrupt("ends early"))?;
                out[offset..offset + run].copy_from_slice(data);
            }
            None => {
                let value = byte(at - 1)?;
                out[offset..offset + run].fill(value);
            }
        }
    }
}

/// The variable-length number UPS and BPS use (7 bits a byte, the last one flagged by its top
/// bit, each continuation adding one so no number has two encodings).
fn varint(patch: &[u8], at: &mut usize) -> Result<u64, PatchError> {
    let mut value = 0u64;
    let mut shift = 1u64;
    loop {
        let byte = *patch.get(*at).ok_or(PatchError::Corrupt("ends early"))?;
        *at += 1;
        value = value
            .checked_add(
                u64::from(byte & 0x7f)
                    .checked_mul(shift)
                    .ok_or(PatchError::Corrupt("number too large"))?,
            )
            .ok_or(PatchError::Corrupt("number too large"))?;
        if byte & 0x80 != 0 {
            return Ok(value);
        }
        shift = shift.checked_shl(7).ok_or(PatchError::Corrupt("number too large"))?;
        value = value.checked_add(shift).ok_or(PatchError::Corrupt("number too large"))?;
    }
}

fn size(value: u64) -> Result<usize, PatchError> {
    // Nothing a console ROM needs is larger; a bigger size is a damaged patch.
    const MAX: u64 = 1 << 30;
    if value > MAX {
        return Err(PatchError::Corrupt("size too large"));
    }
    Ok(value as usize)
}

/// The three checksums at the end of a UPS or BPS patch: source, target, patch.
fn footer(patch: &[u8]) -> Result<(u32, u32), PatchError> {
    if patch.len() < 12 {
        return Err(PatchError::Corrupt("too short"));
    }
    let word = |i: usize| u32::from_le_bytes(patch[i..i + 4].try_into().expect("4 bytes"));
    let end = patch.len();
    if crc32(&patch[..end - 4]) != word(end - 4) {
        return Err(PatchError::PatchChecksum);
    }
    Ok((word(end - 12), word(end - 8)))
}

fn ups(source: &[u8], patch: &[u8]) -> Result<Vec<u8>, PatchError> {
    let (source_crc, target_crc) = footer(patch)?;
    let mut at = 4;
    let source_size = size(varint(patch, &mut at)?)?;
    let target_size = size(varint(patch, &mut at)?)?;
    // A UPS patch works either way round: the input may be its target.
    let input_crc = crc32(source);
    let (output_crc, output_size) = if source.len() == source_size && input_crc == source_crc {
        (target_crc, target_size)
    } else if source.len() == target_size && input_crc == target_crc {
        (source_crc, source_size)
    } else {
        return Err(PatchError::WrongSource);
    };
    let mut out = source.to_vec();
    out.resize(output_size, 0);
    let body_end = patch.len() - 12;
    let mut offset = 0usize;
    while at < body_end {
        offset = offset
            .checked_add(size(varint(patch, &mut at)?)?)
            .ok_or(PatchError::Corrupt("offset too large"))?;
        loop {
            let xor = *patch.get(at).ok_or(PatchError::Corrupt("ends early"))?;
            at += 1;
            if xor == 0 {
                offset += 1;
                break;
            }
            if offset < out.len() {
                out[offset] ^= xor;
            }
            offset += 1;
        }
    }
    if crc32(&out) != output_crc {
        return Err(PatchError::TargetChecksum);
    }
    Ok(out)
}

fn bps(source: &[u8], patch: &[u8]) -> Result<Vec<u8>, PatchError> {
    let (source_crc, target_crc) = footer(patch)?;
    if crc32(source) != source_crc {
        return Err(PatchError::WrongSource);
    }
    let mut at = 4;
    let source_size = size(varint(patch, &mut at)?)?;
    let target_size = size(varint(patch, &mut at)?)?;
    let metadata = size(varint(patch, &mut at)?)?;
    at = at.checked_add(metadata).ok_or(PatchError::Corrupt("metadata"))?;
    if source.len() != source_size {
        return Err(PatchError::WrongSource);
    }
    let body_end = patch.len() - 12;
    let mut out = Vec::with_capacity(target_size);
    let (mut source_rel, mut target_rel) = (0i64, 0i64);
    let signed = |value: u64| -> i64 {
        let magnitude = (value >> 1) as i64;
        if value & 1 == 1 { -magnitude } else { magnitude }
    };
    while at < body_end {
        let data = varint(patch, &mut at)?;
        let length = size((data >> 2) + 1)?;
        match data & 3 {
            // SourceRead: the source's bytes at the same place.
            0 => {
                let from = out.len();
                let bytes =
                    source.get(from..from + length).ok_or(PatchError::Corrupt("source read"))?;
                out.extend_from_slice(bytes);
            }
            // TargetRead: bytes from the patch.
            1 => {
                let bytes = patch.get(at..at + length).ok_or(PatchError::Corrupt("ends early"))?;
                at += length;
                out.extend_from_slice(bytes);
            }
            // SourceCopy: bytes from anywhere in the source.
            2 => {
                source_rel += signed(varint(patch, &mut at)?);
                let from =
                    usize::try_from(source_rel).map_err(|_| PatchError::Corrupt("source copy"))?;
                let bytes =
                    source.get(from..from + length).ok_or(PatchError::Corrupt("source copy"))?;
                out.extend_from_slice(bytes);
                source_rel += length as i64;
            }
            // TargetCopy: bytes already written (byte by byte: the ranges may overlap).
            _ => {
                target_rel += signed(varint(patch, &mut at)?);
                let mut from =
                    usize::try_from(target_rel).map_err(|_| PatchError::Corrupt("target copy"))?;
                for _ in 0..length {
                    let byte = *out.get(from).ok_or(PatchError::Corrupt("target copy"))?;
                    out.push(byte);
                    from += 1;
                }
                target_rel = from as i64;
            }
        }
        if out.len() > target_size {
            return Err(PatchError::Corrupt("writes past the end"));
        }
    }
    if out.len() != target_size || crc32(&out) != target_crc {
        return Err(PatchError::TargetChecksum);
    }
    Ok(out)
}

/// CRC-32 (IEEE), as UPS and BPS use.
pub(crate) fn crc32(bytes: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut table = [0u32; 256];
        for (i, entry) in table.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *entry = c;
        }
        table
    });
    !bytes.iter().fold(!0u32, |c, &b| table[((c ^ u32::from(b)) & 0xff) as usize] ^ (c >> 8))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(mut value: u64, out: &mut Vec<u8>) {
        loop {
            let byte = (value & 0x7f) as u8;
            value >>= 7;
            if value == 0 {
                out.push(byte | 0x80);
                return;
            }
            out.push(byte);
            value -= 1;
        }
    }

    fn finish(mut patch: Vec<u8>, source: &[u8], target: &[u8]) -> Vec<u8> {
        patch.extend_from_slice(&crc32(source).to_le_bytes());
        patch.extend_from_slice(&crc32(target).to_le_bytes());
        let crc = crc32(&patch);
        patch.extend_from_slice(&crc.to_le_bytes());
        patch
    }

    #[test]
    fn crc32_is_the_ieee_one() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn varints_round_trip() {
        for value in [0, 1, 127, 128, 255, 16_511, 16_512, 1 << 20, (1 << 30) + 12_345] {
            let mut bytes = Vec::new();
            encode(value, &mut bytes);
            let mut at = 0;
            assert_eq!(varint(&bytes, &mut at), Ok(value), "{value}");
            assert_eq!(at, bytes.len());
        }
    }

    #[test]
    fn ips_writes_records_runs_and_grows_and_truncates() {
        let source = b"HELLO WORLD".to_vec();
        let mut patch = b"PATCH".to_vec();
        patch.extend_from_slice(&[0, 0, 0, 0, 2, b'J', b'E']); // "JELLO"
        patch.extend_from_slice(&[0, 0, 6, 0, 0, 0, 3, b'*']); // a run: "***LD"
        patch.extend_from_slice(&[0, 0, 12, 0, 1, b'!']); // past the end: grows
        patch.extend_from_slice(b"EOF");
        assert_eq!(apply(&source, &patch).unwrap(), b"JELLO ***LD\0!");
        patch.extend_from_slice(&[0, 0, 5]); // truncated to five bytes
        assert_eq!(apply(&source, &patch).unwrap(), b"JELLO");
        assert_eq!(apply(&source, b"PATCH\0\0"), Err(PatchError::Corrupt("ends early")));
    }

    #[test]
    fn ups_xors_and_checks_both_ends() {
        let source = b"HELLO WORLD".to_vec();
        let target = b"HELLO THERE!".to_vec();
        let mut patch = b"UPS1".to_vec();
        encode(source.len() as u64, &mut patch);
        encode(target.len() as u64, &mut patch);
        // One block at offset 6: the differing bytes, XORed, then a 0.
        encode(6, &mut patch);
        let mut padded = source.clone();
        padded.resize(target.len(), 0);
        for i in 6..target.len() {
            patch.push(padded[i] ^ target[i]);
        }
        patch.push(0);
        let patch = finish(patch, &source, &target);
        assert_eq!(apply(&source, &patch).unwrap(), target);
        assert_eq!(apply(b"HELLO EARTH", &patch), Err(PatchError::WrongSource));
        let mut damaged = patch.clone();
        damaged[7] ^= 1;
        assert_eq!(apply(&source, &damaged), Err(PatchError::PatchChecksum));
    }

    #[test]
    fn bps_reads_and_copies_from_source_patch_and_target() {
        let source = b"ABCDEFGH".to_vec();
        // "ABCD" (SourceRead) + "xy" (TargetRead) + "GH" (SourceCopy from 6) + "GHGH"
        // (TargetCopy from 6: it reads bytes it is writing, so the ranges overlap).
        let target = b"ABCDxyGHGHGH".to_vec();
        let mut patch = b"BPS1".to_vec();
        encode(source.len() as u64, &mut patch);
        encode(target.len() as u64, &mut patch);
        encode(0, &mut patch); // no metadata
        encode((4 - 1) << 2, &mut patch); // SourceRead
        encode(((2 - 1) << 2) | 1, &mut patch);
        patch.extend_from_slice(b"xy");
        encode(((2 - 1) << 2) | 2, &mut patch);
        encode(6 << 1, &mut patch); // source offset +6
        encode(((4 - 1) << 2) | 3, &mut patch);
        encode(6 << 1, &mut patch); // target offset +6
        let patch = finish(patch, &source, &target);
        assert_eq!(apply(&source, &patch).unwrap(), target);
        assert_eq!(apply(b"ABCDEFGI", &patch), Err(PatchError::WrongSource));
    }

    #[test]
    fn unknown_files_are_refused() {
        assert_eq!(apply(b"rom", b"not a patch"), Err(PatchError::UnknownFormat));
    }

    #[test]
    fn a_patched_copy_keeps_the_extension_and_is_named_for_the_patch() {
        let dir = std::env::temp_dir().join(format!("sovemu-patch-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let rom = dir.join("Game (Japan).sfc");
        std::fs::write(&rom, b"HELLO").unwrap();
        let ips = dir.join("Game (Japan).ips");
        let mut patch = b"PATCH".to_vec();
        patch.extend_from_slice(&[0, 0, 0, 0, 1, b'J']);
        patch.extend_from_slice(b"EOF");
        std::fs::write(&ips, patch).unwrap();
        let patched = PatchedRom::write(&rom, &ips, "English").unwrap();
        assert_eq!(patched.path.file_name().unwrap(), "Game (Japan) [English].sfc");
        assert_eq!(std::fs::read(&patched.path).unwrap(), b"JELLO");
        let folder = patched.path.parent().unwrap().to_owned();
        drop(patched);
        assert!(!folder.exists(), "removed with the session");
        std::fs::remove_dir_all(&dir).ok();
    }
}
