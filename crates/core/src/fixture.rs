//! Builds tiny Minecraft worlds for tests (not used by the app itself).

use std::ops::Range;

struct W(Vec<u8>);

impl W {
    fn name(&mut self, tag: u8, name: &str) {
        self.0.push(tag);
        self.0.extend((name.len() as u16).to_be_bytes());
        self.0.extend(name.as_bytes());
    }
    fn str_payload(&mut self, v: &str) {
        self.0.extend((v.len() as u16).to_be_bytes());
        self.0.extend(v.as_bytes());
    }
    fn string(&mut self, name: &str, v: &str) {
        self.name(8, name);
        self.str_payload(v);
    }
    fn int(&mut self, name: &str, v: i32) {
        self.name(3, name);
        self.0.extend(v.to_be_bytes());
    }
    fn byte(&mut self, name: &str, v: i8) {
        self.name(1, name);
        self.0.push(v as u8);
    }
    fn end(&mut self) {
        self.0.push(0);
    }
}

fn section_nbt(sy: i32, block: &dyn Fn(usize, i32, usize) -> &'static str, biome: &str) -> Option<Vec<u8>> {
    let mut palette: Vec<&'static str> = Vec::new();
    let mut idx: Vec<usize> = Vec::with_capacity(4096);
    for ly in 0..16 {
        for lz in 0..16 {
            for lx in 0..16 {
                let b = block(lx, sy * 16 + ly as i32, lz);
                let p = match palette.iter().position(|x| *x == b) {
                    Some(p) => p,
                    None => {
                        palette.push(b);
                        palette.len() - 1
                    }
                };
                idx.push(p);
            }
        }
    }
    if palette.len() == 1 && palette[0] == "minecraft:air" {
        return None;
    }

    let mut s = W(Vec::new());
    s.byte("Y", sy as i8);
    s.name(10, "block_states");
    s.name(9, "palette");
    s.0.push(10);
    s.0.extend((palette.len() as i32).to_be_bytes());
    for p in &palette {
        s.string("Name", p);
        s.end();
    }
    if palette.len() > 1 {
        let bits = (usize::BITS - (palette.len() - 1).leading_zeros()).max(4) as usize;
        let per = 64 / bits;
        let mut longs = vec![0u64; (4096 + per - 1) / per];
        for (i, p) in idx.iter().enumerate() {
            longs[i / per] |= (*p as u64) << ((i % per) * bits);
        }
        s.name(12, "data");
        s.0.extend((longs.len() as i32).to_be_bytes());
        for l in longs {
            s.0.extend(l.to_be_bytes());
        }
    }
    s.end();
    s.name(10, "biomes");
    s.name(9, "palette");
    s.0.push(8);
    s.0.extend(1i32.to_be_bytes());
    s.str_payload(biome);
    s.end();
    s.end();
    Some(s.0)
}

/// NBT of one fully generated chunk. `block(x, y, z)` gives the block at chunk-local x/z and
/// world y; `sections` is the range of section indices to write (-4..20 for the Overworld).
/// Sections that are only air are left out, like the game does.
pub fn chunk_nbt(sections: Range<i32>, block: &dyn Fn(usize, i32, usize) -> &'static str, biome: &str) -> Vec<u8> {
    let mut w = W(Vec::new());
    w.name(10, "");
    w.int("DataVersion", 4000);
    w.string("Status", "minecraft:full");
    // Data the renderer has to skip over
    w.name(5, "junk_float");
    w.0.extend(1.5f32.to_be_bytes());
    w.name(10, "Heightmaps");
    w.name(12, "WORLD_SURFACE");
    w.0.extend(2i32.to_be_bytes());
    w.0.extend(0i64.to_be_bytes());
    w.0.extend(0i64.to_be_bytes());
    w.end();
    w.name(9, "block_entities");
    w.0.push(0);
    w.0.extend(0i32.to_be_bytes());

    let list: Vec<Vec<u8>> = sections.filter_map(|sy| section_nbt(sy, block, biome)).collect();
    w.name(9, "sections");
    w.0.push(10);
    w.0.extend((list.len() as i32).to_be_bytes());
    for s in list {
        w.0.extend(s);
    }
    w.end();
    w.0
}

/// A region file holding the given chunks `(local x, local z, chunk NBT)`, stored uncompressed.
pub fn region_bytes(chunks: &[(usize, usize, Vec<u8>)]) -> Vec<u8> {
    let mut header = vec![0u8; 8192];
    let mut body: Vec<u8> = Vec::new();
    let mut sector = 2u32;
    for (cx, cz, nbt) in chunks {
        let mut rec = Vec::new();
        rec.extend(((nbt.len() + 1) as u32).to_be_bytes());
        rec.push(3);
        rec.extend(nbt);
        let sectors = (rec.len() + 4095) / 4096;
        rec.resize(sectors * 4096, 0);
        let idx = cz * 32 + cx;
        let entry = (sector << 8) | sectors as u32;
        header[idx * 4..idx * 4 + 4].copy_from_slice(&entry.to_be_bytes());
        body.extend(rec);
        sector += sectors as u32;
    }
    header.extend(body);
    header
}

/// A region with one flat chunk in its north-west corner: `top` at y=64, stone below.
pub fn flat_region(top: &'static str) -> Vec<u8> {
    let nbt = chunk_nbt(
        -4..20,
        &move |_x, y, _z| {
            if y > 64 {
                "minecraft:air"
            } else if y == 64 {
                top
            } else {
                "minecraft:stone"
            }
        },
        "minecraft:plains",
    );
    region_bytes(&[(0, 0, nbt)])
}
