use anyhow::{bail, Result};
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use flate2::Compression;
use std::io::Read;
use crate::math::{Vec3, Vec4};

/// "RSC7": the header on every GTA V resource file (.ydr/.ytd/.ybn/.ynv…).
pub const RSC7_MAGIC: u32 = 0x37435352;
/// "RSC8": the Gen9 resource header, not supported by this crate.
pub const RSC8_MAGIC: u32 = 0x38435352;

/// The resource version encoded in the two RSC7 flag words (system nibble
/// high, graphics nibble low), e.g. 165 for a .ydr.
pub fn resource_version_from_flags(sys_flags: u32, gfx_flags: u32) -> u32 {
    let sv = (sys_flags  >> 28) & 0xF;
    let gv = (gfx_flags  >> 28) & 0xF;
    (sv << 4) | gv
}

/// Decodes an RSC7 flag word into the byte size of the section it describes.
pub fn resource_size_from_flags(flags: u32) -> usize {
    let s0 = ((flags >> 27) & 0x1)  << 0;
    let s1 = ((flags >> 26) & 0x1)  << 1;
    let s2 = ((flags >> 25) & 0x1)  << 2;
    let s3 = ((flags >> 24) & 0x1)  << 3;
    let s4 = ((flags >> 17) & 0x7F) << 4;
    let s5 = ((flags >> 11) & 0x3F) << 5;
    let s6 = ((flags >> 7)  & 0xF)  << 6;
    let s7 = ((flags >> 5)  & 0x3)  << 7;
    let s8 = ((flags >> 4)  & 0x1)  << 8;
    let ss = (flags & 0xF) as usize;
    let base_size = 0x200usize << ss;
    base_size * (s0 + s1 + s2 + s3 + s4 + s5 + s6 + s7 + s8) as usize
}

pub const SYSTEM_BASE: u64 = 0x5000_0000;
pub const GRAPHICS_BASE: u64 = 0x6000_0000;

// ─── Internal virtual-memory reader ──────────────────────────────────────────

pub struct ResReader<'a> {
    pub system:   &'a [u8],
    pub graphics: &'a [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    System,
    Graphics,
}

/// Header of a `atArray`/pointer-list style structure: a pointer to the
/// backing array, followed by a `u16` count and a `u16` capacity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointerListHeader {
    pub pointer: u64,
    pub count: u16,
    pub capacity: u16,
}

impl<'a> ResReader<'a> {
    /// Which section a virtual address points into, and the offset within it.
    /// Both bases share bit 30; bit 29 marks graphics and bit 28 system. An
    /// address with all three set counts as graphics, as it always has.
    fn locate(&self, va: u64) -> Option<(Section, usize)> {
        if va & GRAPHICS_BASE == GRAPHICS_BASE {
            Some((Section::Graphics, (va - GRAPHICS_BASE) as usize))
        } else if va & SYSTEM_BASE == SYSTEM_BASE {
            Some((Section::System, (va - SYSTEM_BASE) as usize))
        } else {
            None
        }
    }

    fn section(&self, which: Section) -> &'a [u8] {
        match which {
            Section::System => self.system,
            Section::Graphics => self.graphics,
        }
    }

    pub fn resolve(&self, va: u64, len: usize) -> Option<&'a [u8]> {
        if va == 0 { return None; }
        let (section, off) = self.locate(va)?;
        self.section(section).get(off..off.checked_add(len)?)
    }

    /// Like [`Self::resolve`], but distinguishes a null pointer (`va == 0`,
    /// returns `Some(None)`) from an out-of-bounds pointer (`None`).
    pub fn resolve_optional(&self, va: u64, len: usize) -> Option<Option<&'a [u8]>> {
        if va == 0 {
            return Some(None);
        }
        self.resolve(va, len).map(Some)
    }

    pub fn read_u16_list(&self, va: u64, count: usize) -> Option<Vec<u16>> {
        if count == 0 || va == 0 {
            return Some(Vec::new());
        }
        let bytes = self.resolve(va, count.checked_mul(2)?)?;
        Some(bytes.chunks_exact(2).map(|c| u16_le(c, 0)).collect())
    }

    pub fn read_u32_list(&self, va: u64, count: usize) -> Option<Vec<u32>> {
        if count == 0 || va == 0 {
            return Some(Vec::new());
        }
        let bytes = self.resolve(va, count.checked_mul(4)?)?;
        Some(bytes.chunks_exact(4).map(|c| u32_le(c, 0)).collect())
    }

    pub fn read_u64_list(&self, va: u64, count: usize) -> Option<Vec<u64>> {
        if count == 0 || va == 0 {
            return Some(Vec::new());
        }
        let bytes = self.resolve(va, count.checked_mul(8)?)?;
        Some(bytes.chunks_exact(8).map(|c| u64_le(c, 0)).collect())
    }

    /// Reads a 16-byte pointer-list header: pointer@0, count@8, capacity@10.
    pub fn read_pointer_list_header(&self, va: u64) -> Option<PointerListHeader> {
        let bytes = self.resolve(va, 16)?;
        Some(PointerListHeader {
            pointer: u64_le(bytes, 0),
            count: u16_le(bytes, 8),
            capacity: u16_le(bytes, 10),
        })
    }

    /// Longest string this reads before giving up on finding a terminating
    /// NUL. Real names are short; a runaway scan across the rest of the
    /// section usually means the pointer is bogus, so it's treated as an
    /// unresolved string (`None`) rather than returned truncated.
    const MAX_STRING_LEN: usize = 256;

    /// Names only ever live in the system section; a graphics-section
    /// address is treated as unresolved.
    pub fn string_at(&self, va: u64) -> Option<String> {
        let (Section::System, off) = self.locate(va)? else {
            return None;
        };
        let slice = self.system.get(off..)?;
        let scan_len = slice.len().min(Self::MAX_STRING_LEN);
        let end = slice[..scan_len].iter().position(|&b| b == 0)?;
        Some(String::from_utf8_lossy(&slice[..end]).into_owned())
    }
}

pub fn u16_le(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes(b[off..off + 2].try_into().unwrap_or([0; 2]))
}
pub fn u32_le(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(b[off..off + 4].try_into().unwrap_or([0; 4]))
}
pub fn u64_le(b: &[u8], off: usize) -> u64 {
    u64::from_le_bytes(b[off..off + 8].try_into().unwrap_or([0; 8]))
}
pub fn f32_le(b: &[u8], off: usize) -> f32 {
    f32::from_le_bytes(b[off..off + 4].try_into().unwrap_or([0; 4]))
}
pub fn vec3_le(b: &[u8], off: usize) -> Vec3 {
    Vec3::new(f32_le(b, off), f32_le(b, off + 4), f32_le(b, off + 8))
}
pub fn vec4_le(b: &[u8], off: usize) -> Vec4 {
    Vec4::new(f32_le(b, off), f32_le(b, off + 4), f32_le(b, off + 8), f32_le(b, off + 12))
}

/// Helper to decompress and prepare RSC7 resource sections.
pub fn prepare_rsc7(data: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    if data.len() < 16 {
        bail!("RSC7 data too short");
    }

    let magic = u32::from_le_bytes(data[0..4].try_into().unwrap());
    if magic != RSC7_MAGIC {
        bail!("Not an RSC7 file (magic = 0x{:08X})", magic);
    }

    let system_flags  = u32::from_le_bytes(data[8..12].try_into().unwrap());
    let graphics_flags = u32::from_le_bytes(data[12..16].try_into().unwrap());

    let sys_size  = resource_size_from_flags(system_flags);
    let gfx_size  = resource_size_from_flags(graphics_flags);
    let body      = &data[16..];

    // Most resources are deflated, but a few are stored raw. Telling the two
    // apart by whether inflate succeeds is fine; what matters is not confusing
    // a *corrupt* stream for a stored one, because feeding the still-compressed
    // bytes on as though they were the resource produces wild pointers far
    // downstream instead of naming the real problem here.
    let decompressed = {
        let mut out = Vec::new();
        match DeflateDecoder::new(body).read_to_end(&mut out) {
            Ok(_) if !out.is_empty() => out,
            Ok(_) => body.to_vec(),
            // Never looked like deflate at all — treat it as stored.
            Err(_) if out.is_empty() => body.to_vec(),
            Err(_) => bail!(
                "corrupt deflate stream: inflated {} of an expected {} bytes before failing",
                out.len(),
                sys_size + gfx_size
            ),
        }
    };

    if decompressed.len() < sys_size {
        bail!(
            "Decompressed size {} < expected system size {}",
            decompressed.len(), sys_size
        );
    }

    let system = decompressed[..sys_size].to_vec();
    let graphics = if decompressed.len() >= sys_size + gfx_size {
        decompressed[sys_size..sys_size + gfx_size].to_vec()
    } else {
        decompressed[sys_size..].to_vec()
    };

    Ok((system, graphics))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a system buffer with a 16-byte pointer-list header at offset 0
    /// (pointer -> 0x100 within the system section, count=3, capacity=4)
    /// followed by three little-endian u32s at 0x50000100.
    fn build_system_buffer() -> Vec<u8> {
        let mut sys = vec![0u8; 0x200];

        // Pointer-list header at offset 0.
        let array_va = SYSTEM_BASE + 0x100;
        sys[0..8].copy_from_slice(&array_va.to_le_bytes());
        sys[8..10].copy_from_slice(&3u16.to_le_bytes());
        sys[10..12].copy_from_slice(&4u16.to_le_bytes());

        // Three u32s at 0x100.
        sys[0x100..0x104].copy_from_slice(&11u32.to_le_bytes());
        sys[0x104..0x108].copy_from_slice(&22u32.to_le_bytes());
        sys[0x108..0x10C].copy_from_slice(&33u32.to_le_bytes());

        sys
    }

    #[test]
    fn resolve_optional_null_out_of_bounds_and_valid() {
        let sys = build_system_buffer();
        let reader = ResReader { system: &sys, graphics: &[] };

        // va == 0 -> Some(None)
        assert_eq!(reader.resolve_optional(0, 4), Some(None));

        // Out of bounds -> None
        let far_va = SYSTEM_BASE + sys.len() as u64 + 0x1000;
        assert_eq!(reader.resolve_optional(far_va, 4), None);

        // Valid -> Some(Some(bytes))
        let array_va = SYSTEM_BASE + 0x100;
        let resolved = reader.resolve_optional(array_va, 4).expect("should resolve");
        let bytes = resolved.expect("should be Some(bytes)");
        assert_eq!(u32_le(bytes, 0), 11);
    }

    #[test]
    fn read_u32_list_reads_values() {
        let sys = build_system_buffer();
        let reader = ResReader { system: &sys, graphics: &[] };

        let array_va = SYSTEM_BASE + 0x100;
        let values = reader.read_u32_list(array_va, 3).expect("should read list");
        assert_eq!(values, vec![11, 22, 33]);

        // count == 0 -> empty vec, even for a null va.
        assert_eq!(reader.read_u32_list(0, 0), Some(Vec::new()));
    }

    #[test]
    fn read_lists_reject_counts_that_would_overflow_the_byte_length() {
        let sys = build_system_buffer();
        let reader = ResReader { system: &sys, graphics: &[] };
        let array_va = SYSTEM_BASE + 0x100;

        // On a 32-bit `usize` (wasm32), `count * N` wraps around instead of
        // overflowing, so pick a count that overflows even a 64-bit `usize`
        // multiplication to make the assertion hold on every target.
        let huge_count = usize::MAX / 4 + 1;

        assert_eq!(reader.read_u16_list(array_va, huge_count), None);
        assert_eq!(reader.read_u32_list(array_va, huge_count), None);
        assert_eq!(reader.read_u64_list(array_va, huge_count), None);
    }

    #[test]
    fn resolve_rejects_offset_length_overflow_instead_of_panicking() {
        let reader = ResReader { system: &[], graphics: &[] };

        // A `va` with bit 29 (system) or bit 30 (graphics) set near `u64::MAX`
        // plus a huge `len` used to overflow `off + len` in debug builds.
        assert_eq!(reader.resolve(u64::MAX, usize::MAX), None);
    }

    #[test]
    fn string_at_gives_up_past_the_scan_cap_instead_of_returning_untruncated() {
        // No NUL anywhere in a buffer larger than the scan cap -> None, not a
        // huge (or truncated) string.
        let sys = vec![b'A'; 512];
        let reader = ResReader { system: &sys, graphics: &[] };
        assert_eq!(reader.string_at(SYSTEM_BASE), None);

        // A NUL just past the cap still isn't found.
        let mut sys = vec![b'A'; 300];
        sys[300 - 1] = 0; // NUL at index 299, past the 256-byte scan window
        let reader = ResReader { system: &sys, graphics: &[] };
        assert_eq!(reader.string_at(SYSTEM_BASE), None);

        // A NUL within the cap still works normally.
        let mut sys = vec![b'A'; 10];
        sys[5] = 0;
        let reader = ResReader { system: &sys, graphics: &[] };
        assert_eq!(reader.string_at(SYSTEM_BASE), Some("AAAAA".to_string()));
    }

    #[test]
    fn read_pointer_list_header_reads_fields() {
        let sys = build_system_buffer();
        let reader = ResReader { system: &sys, graphics: &[] };

        let header = reader.read_pointer_list_header(SYSTEM_BASE).expect("should read header");
        assert_eq!(header.pointer, SYSTEM_BASE + 0x100);
        assert_eq!(header.count, 3);
        assert_eq!(header.capacity, 4);
    }
}

// ─── writing ─────────────────────────────────────────────────────────────────

/// The RSC7 flag word describing the smallest page set (largest pages first,
/// per-size counts capped as the format caps them) that holds `size` bytes.
/// The virtual size it encodes is what [`resource_size_from_flags`] returns,
/// so callers pad their section to that.
pub fn rsc7_flags_for_size(size: usize) -> Result<u32> {
    for ss in 0u32..16 {
        let base = 0x200usize << ss;
        let mut units = size.div_ceil(base);
        // (shift into the flag word, page size in units, max count)
        let fields: [(u32, usize, usize); 9] = [
            (4, 256, 1), (5, 128, 3), (7, 64, 15), (11, 32, 63), (17, 16, 127),
            (24, 8, 1), (25, 4, 1), (26, 2, 1), (27, 1, 1),
        ];
        let mut flags = ss;
        for (shift, page, cap) in fields {
            let n = (units / page).min(cap);
            units -= n * page;
            flags |= (n as u32) << shift;
        }
        if units == 0 {
            return Ok(flags);
        }
    }
    bail!("{size} bytes is too large for an RSC7 section");
}

/// The flag word for `count` pages of exactly `page_size` bytes each (a
/// power of two times 0x200). Every block in the section must then sit
/// inside one page: the game maps pages as separate allocations.
pub fn rsc7_flags_for_pages(page_size: usize, count: usize) -> Result<u32> {
    // (shift into the flag word, page size in base units, max count)
    let fields: [(u32, usize, usize); 9] = [
        (27, 1, 1), (26, 2, 1), (25, 4, 1), (24, 8, 1), (17, 16, 127), (11, 32, 63), (7, 64, 15), (5, 128, 3), (4, 256, 1),
    ];
    for ss in 0u32..16 {
        let base = 0x200usize << ss;
        if page_size < base { break; }
        for (shift, units, cap) in fields {
            if base * units == page_size && count <= cap {
                return Ok(ss | ((count as u32) << shift));
            }
        }
    }
    bail!("{count} pages of {page_size} bytes cannot be described by RSC7 flags");
}

/// [`build_rsc7`] for a system section laid out in equal pages of
/// `page_size` bytes (no graphics section): the section is padded to a
/// whole number of pages and the flags describe exactly those pages.
pub fn build_rsc7_paged(version: u32, system: &[u8], page_size: usize) -> Result<Vec<u8>> {
    let pages = system.len().div_ceil(page_size).max(1);
    let sys_flags = rsc7_flags_for_pages(page_size, pages)? | ((version >> 4) & 0xF) << 28;
    let gfx_flags = (version & 0xF) << 28;
    let mut body = system.to_vec();
    body.resize(pages * page_size, 0);
    Ok(wrap_rsc7(version, sys_flags, gfx_flags, &body))
}

/// Wraps a system (and optional graphics) section in an RSC7 header with the
/// given resource version and a deflated body, padding each section to the
/// page layout its flags describe. Version is the pair of nibbles
/// [`resource_version_from_flags`] reads back (e.g. 2 for a .ynv, 165 for a .ydr).
pub fn build_rsc7(version: u32, system: &[u8], graphics: &[u8]) -> Vec<u8> {
    let sys_flags = rsc7_flags_for_size(system.len().max(1)).expect("section fits") | ((version >> 4) & 0xF) << 28;
    let gfx_flags = if graphics.is_empty() { 0 } else { rsc7_flags_for_size(graphics.len()).expect("section fits") }
        | (version & 0xF) << 28;
    let mut body = Vec::with_capacity(resource_size_from_flags(sys_flags) + resource_size_from_flags(gfx_flags));
    body.extend_from_slice(system);
    body.resize(resource_size_from_flags(sys_flags), 0);
    body.extend_from_slice(graphics);
    body.resize(resource_size_from_flags(sys_flags) + resource_size_from_flags(gfx_flags), 0);

    wrap_rsc7(version, sys_flags, gfx_flags, &body)
}

fn wrap_rsc7(version: u32, sys_flags: u32, gfx_flags: u32, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&RSC7_MAGIC.to_le_bytes());
    out.extend_from_slice(&version.to_le_bytes());
    out.extend_from_slice(&sys_flags.to_le_bytes());
    out.extend_from_slice(&gfx_flags.to_le_bytes());
    let mut enc = DeflateEncoder::new(out, Compression::best());
    std::io::Write::write_all(&mut enc, body).expect("writing to a Vec cannot fail");
    enc.finish().expect("writing to a Vec cannot fail")
}

#[cfg(test)]
mod writer_tests {
    use super::*;

    #[test]
    fn flags_round_trip_through_size() {
        for size in [1usize, 512, 8192, 24576 + 180224 + 32768, 237568, 1 << 20, 3_000_001] {
            let flags = rsc7_flags_for_size(size).unwrap();
            let virt = resource_size_from_flags(flags);
            assert!(virt >= size, "{size}: {virt}");
            assert!(virt < size + (0x200usize << (flags & 0xF)) * 256, "{size}: wasteful {virt}");
        }
        // The retail navmesh[108][96].ynv layout.
        assert_eq!(resource_size_from_flags(0x0006_5880), 237568);
    }

    #[test]
    fn page_flags_describe_equal_pages() {
        let flags = rsc7_flags_for_pages(16384, 8).unwrap();
        assert_eq!(resource_size_from_flags(flags), 8 * 16384);
        let file = build_rsc7_paged(2, &[7u8; 40000], 16384).unwrap();
        let sys_flags = u32::from_le_bytes(file[8..12].try_into().unwrap());
        assert_eq!(resource_size_from_flags(sys_flags), 3 * 16384);
        assert!(rsc7_flags_for_pages(16384, 10_000).is_err());
    }

    #[test]
    fn build_rsc7_round_trips() {
        let system: Vec<u8> = (0..5000u32).map(|i| (i * 7 % 251) as u8).collect();
        let file = build_rsc7(2, &system, &[]);
        assert_eq!(u32::from_le_bytes(file[0..4].try_into().unwrap()), RSC7_MAGIC);
        let sys_flags = u32::from_le_bytes(file[8..12].try_into().unwrap());
        let gfx_flags = u32::from_le_bytes(file[12..16].try_into().unwrap());
        assert_eq!(resource_version_from_flags(sys_flags, gfx_flags), 2);
        let (sys, gfx) = prepare_rsc7(&file).unwrap();
        assert_eq!(&sys[..system.len()], &system[..]);
        assert!(gfx.is_empty());
    }
}
