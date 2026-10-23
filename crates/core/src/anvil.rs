//! Reading Minecraft's Anvil region files (`r.X.Z.mca`) and chunk data (1.18+ format).

use crate::{inflate, nbt};
use std::{fs, io, path::Path};

pub struct RegionFile {
    data: Vec<u8>,
}

impl RegionFile {
    pub fn open(path: &Path) -> io::Result<RegionFile> {
        Ok(RegionFile { data: fs::read(path)? })
    }

    pub fn from_bytes(data: Vec<u8>) -> RegionFile {
        RegionFile { data }
    }

    /// Decompressed NBT of the chunk at local position (0..32, 0..32), if it exists.
    pub fn chunk_nbt(&self, cx: usize, cz: usize) -> Result<Option<Vec<u8>>, String> {
        if self.data.len() < 8192 {
            return Ok(None); // empty or brand new region file
        }
        let idx = cz * 32 + cx;
        let e = &self.data[idx * 4..idx * 4 + 4];
        let entry = u32::from_be_bytes([e[0], e[1], e[2], e[3]]);
        let sector = (entry >> 8) as usize;
        if entry == 0 || sector < 2 {
            return Ok(None);
        }
        let start = sector * 4096;
        if start + 5 > self.data.len() {
            return Err("chunk lies outside the region file (truncated file?)".into());
        }
        let len = u32::from_be_bytes([self.data[start], self.data[start + 1], self.data[start + 2], self.data[start + 3]]) as usize;
        let kind = self.data[start + 4];
        if len < 1 {
            return Ok(None);
        }
        let end = start + 4 + len;
        if end > self.data.len() {
            return Err("chunk is cut off (truncated file?)".into());
        }
        if kind & 0x80 != 0 {
            return Err("chunk is stored in an external .mcc file, not supported".into());
        }
        inflate::decompress(kind, &self.data[start + 5..end]).map(Some)
    }
}

pub struct Section {
    /// Section index along Y (block y = `y * 16`).
    pub y: i32,
    pub palette: Vec<String>,
    /// Packed palette indices; empty when the palette has a single entry.
    pub data: Vec<i64>,
    pub biomes: Vec<String>,
    pub biome_data: Vec<i64>,
}

pub struct Chunk {
    /// Sorted from the highest section to the lowest.
    pub sections: Vec<Section>,
}

fn keep(depth: usize, key: &str) -> bool {
    match depth {
        0 => matches!(key, "sections" | "Status" | "Level"),
        1 => matches!(key, "Y" | "block_states" | "biomes"),
        2 => matches!(key, "palette" | "data"),
        3 => key == "Name",
        _ => false,
    }
}

/// Parse a chunk. `Ok(None)` for chunks that are not fully generated or have no sections.
pub fn parse_chunk(bytes: &[u8]) -> Result<Option<Chunk>, String> {
    let root = nbt::parse_root(bytes, &keep)?;
    if root.get("Level").is_some() && root.get("sections").is_none() {
        return Err("chunk is in the pre-1.18 format, which is not supported".into());
    }
    if let Some(status) = root.get("Status").and_then(|s| s.as_str()) {
        if status.strip_prefix("minecraft:").unwrap_or(status) != "full" {
            return Ok(None);
        }
    }
    let Some(list) = root.get("sections").and_then(|s| s.as_list()) else {
        return Ok(None);
    };

    let mut sections = Vec::with_capacity(list.len());
    for s in list {
        let Some(y) = s.get("Y").and_then(|v| v.as_i64()) else { continue };
        let Some(bs) = s.get("block_states") else { continue };
        let palette: Vec<String> = bs
            .get("palette")
            .and_then(|p| p.as_list())
            .map(|l| l.iter().map(|e| e.get("Name").and_then(|n| n.as_str()).unwrap_or("minecraft:air").to_string()).collect())
            .unwrap_or_default();
        if palette.is_empty() {
            continue;
        }
        let data = bs.get("data").and_then(|d| d.as_longs()).map(|l| l.to_vec()).unwrap_or_default();
        let (biomes, biome_data) = match s.get("biomes") {
            Some(b) => (
                b.get("palette")
                    .and_then(|p| p.as_list())
                    .map(|l| l.iter().filter_map(|e| e.as_str().map(|x| x.to_string())).collect())
                    .unwrap_or_default(),
                b.get("data").and_then(|d| d.as_longs()).map(|l| l.to_vec()).unwrap_or_default(),
            ),
            None => (Vec::new(), Vec::new()),
        };
        sections.push(Section { y: y as i32, palette, data, biomes, biome_data });
    }
    sections.sort_by(|a, b| b.y.cmp(&a.y));
    Ok(Some(Chunk { sections }))
}

/// Number of bits per palette index in a block-state container.
pub fn block_bits(palette_len: usize) -> u32 {
    let need = usize::BITS - palette_len.saturating_sub(1).leading_zeros();
    need.max(4)
}

/// Number of bits per palette index in a biome container.
pub fn biome_bits(palette_len: usize) -> u32 {
    let need = usize::BITS - palette_len.saturating_sub(1).leading_zeros();
    need.max(1)
}

/// Read entry `i` of a packed array (entries never straddle two longs since 1.16).
#[inline]
pub fn packed_get(data: &[i64], bits: u32, i: usize) -> usize {
    let per = (64 / bits) as usize;
    let long = match data.get(i / per) {
        Some(l) => *l as u64,
        None => return 0,
    };
    ((long >> ((i % per) as u32 * bits)) & ((1u64 << bits) - 1)) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bit_widths() {
        assert_eq!(block_bits(1), 4);
        assert_eq!(block_bits(16), 4);
        assert_eq!(block_bits(17), 5);
        assert_eq!(block_bits(33), 6);
        assert_eq!(biome_bits(1), 1);
        assert_eq!(biome_bits(2), 1);
        assert_eq!(biome_bits(3), 2);
        assert_eq!(biome_bits(5), 3);
    }

    #[test]
    fn packed_reads() {
        // 4 bits, 16 entries per long: entry i has value i % 16
        let mut l = 0u64;
        for i in 0..16u64 {
            l |= i << (i * 4);
        }
        let d = [l as i64];
        for i in 0..16 {
            assert_eq!(packed_get(&d, 4, i), i);
        }
        // 5 bits: 12 per long, 4 bits unused at the top
        let mut l = 0u64;
        for i in 0..12u64 {
            l |= (i + 17) % 32 << (i * 5);
        }
        let d = [l as i64];
        for i in 0..12 {
            assert_eq!(packed_get(&d, 5, i), (i + 17) % 32);
        }
    }
}
