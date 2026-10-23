//! Draws one region (512 x 512 blocks) as a top-down picture, one pixel per block.
//!
//! For every column the highest visible block is found (looking through air, plants, glass
//! and the like), coloured by block and biome, shaded by the height difference to the
//! neighbours to the north and west (light comes from the north-west), and for water the
//! colour of the floor is blended with the water colour by depth.
//! The Nether is drawn from below its bedrock roof.

use crate::{
    anvil::{self, RegionFile},
    blocks::{self, BiomeColors, Class, Rgb, Tint},
};
use std::collections::{BTreeSet, HashMap};

pub const REGION_PX: usize = 512;
/// The Nether's ceiling is bedrock up to y=127; looking from here finds the open space below.
const NETHER_TOP: i32 = 122;
const NO_HEIGHT: i16 = i16::MIN;

pub struct RegionOut {
    /// `REGION_PX * REGION_PX * 4` bytes, RGBA, row-major (north-west corner first).
    pub rgba: Vec<u8>,
    pub has_pixels: bool,
    pub chunks_ok: u32,
    pub chunks_bad: u32,
    pub first_error: Option<String>,
    /// Block names that matched no colour rule (drawn grey).
    pub unknown: BTreeSet<String>,
}

#[derive(Default)]
struct Cache {
    classes: HashMap<String, Class>,
    biomes: HashMap<String, BiomeColors>,
    unknown: BTreeSet<String>,
}

impl Cache {
    fn class(&mut self, name: &str) -> Class {
        if let Some(c) = self.classes.get(name) {
            return *c;
        }
        let (c, known) = blocks::classify(name);
        if !known {
            self.unknown.insert(name.to_string());
        }
        self.classes.insert(name.to_string(), c);
        c
    }

    fn biome(&mut self, name: &str) -> BiomeColors {
        if let Some(b) = self.biomes.get(name) {
            return *b;
        }
        let b = blocks::biome_colors(name);
        self.biomes.insert(name.to_string(), b);
        b
    }
}

struct Sec<'a> {
    y: i32,
    s: &'a anvil::Section,
    classes: Vec<Class>,
    /// Nothing in this section can be seen from above.
    trivial: bool,
    bits: u32,
    /// Unpacked palette indices, filled the first time a column needs them.
    idx: Option<Vec<u16>>,
    biome_cols: Vec<BiomeColors>,
    bbits: u32,
}

impl<'a> Sec<'a> {
    fn palette_index(&mut self, lx: usize, ly: usize, lz: usize) -> usize {
        if self.classes.len() == 1 {
            return 0;
        }
        if self.idx.is_none() {
            let mut v = Vec::with_capacity(4096);
            for i in 0..4096 {
                v.push(anvil::packed_get(&self.s.data, self.bits, i) as u16);
            }
            self.idx = Some(v);
        }
        self.idx.as_ref().unwrap()[ly * 256 + lz * 16 + lx] as usize
    }

    fn biome_at(&self, lx: usize, ly: usize, lz: usize, default: BiomeColors) -> BiomeColors {
        match self.biome_cols.len() {
            0 => default,
            1 => self.biome_cols[0],
            _ => {
                let i = anvil::packed_get(&self.s.biome_data, self.bbits, (ly / 4) * 16 + (lz / 4) * 4 + lx / 4);
                self.biome_cols.get(i).copied().unwrap_or(default)
            }
        }
    }
}

struct Found {
    y: i32,
    rgb: Rgb,
    water_depth: u8,
    water_rgb: Rgb,
}

fn scale(c: Rgb, f: f32) -> Rgb {
    [(c[0] as f32 * f).min(255.0) as u8, (c[1] as f32 * f).min(255.0) as u8, (c[2] as f32 * f).min(255.0) as u8]
}

fn resolve(rgb: Rgb, tint: Tint, b: &BiomeColors) -> Rgb {
    match tint {
        Tint::None => rgb,
        Tint::Grass => b.grass,
        Tint::Foliage => scale(b.foliage, 0.82),
    }
}

fn scan_column(secs: &mut [Sec], lx: usize, lz: usize, nether: bool, default: BiomeColors) -> Option<Found> {
    let mut water_top: Option<(i32, Rgb)> = None;
    let mut entered = !nether;
    let mut expect = NETHER_TOP;
    let mut fallback: Option<Found> = None;

    for sec in secs.iter_mut() {
        let bottom = sec.y * 16;
        let mut start = 15usize;
        if nether {
            if bottom > NETHER_TOP {
                continue;
            }
            let top = (bottom + 15).min(NETHER_TOP);
            if !entered && top < expect {
                entered = true; // missing sections are open air
            }
            start = (top - bottom) as usize;
        }
        if sec.trivial {
            if nether {
                entered = true;
                expect = bottom - 1;
            }
            continue;
        }

        for ly in (0..=start).rev() {
            let wy = bottom + ly as i32;
            let pi = sec.palette_index(lx, ly, lz);
            let class = sec.classes.get(pi).copied().unwrap_or(Class::Skip);

            if !entered {
                match class {
                    Class::Skip | Class::Water => entered = true,
                    Class::Solid { rgb, tint } => {
                        if fallback.is_none() {
                            let b = sec.biome_at(lx, ly, lz, default);
                            fallback = Some(Found { y: wy, rgb: resolve(rgb, tint, &b), water_depth: 0, water_rgb: [0, 0, 0] });
                        }
                    }
                }
                continue;
            }

            match class {
                Class::Skip => {}
                Class::Water => {
                    if water_top.is_none() {
                        water_top = Some((wy, sec.biome_at(lx, ly, lz, default).water));
                    }
                }
                Class::Solid { rgb, tint } => {
                    let b = sec.biome_at(lx, ly, lz, default);
                    let (depth, wrgb) = match water_top {
                        Some((wt, wc)) => ((wt - wy).clamp(0, 255) as u8, wc),
                        None => (0, [0, 0, 0]),
                    };
                    return Some(Found { y: wy, rgb: resolve(rgb, tint, &b), water_depth: depth, water_rgb: wrgb });
                }
            }
        }
        expect = bottom - 1;
    }

    if let Some((wt, wc)) = water_top {
        // Water all the way down (should not happen): dark deep water.
        return Some(Found { y: wt - 24, rgb: [18, 28, 64], water_depth: 24, water_rgb: wc });
    }
    fallback
}

/// Render one region. Corrupt chunks are skipped and counted, never fatal.
pub fn render_region(region: &RegionFile, nether: bool) -> RegionOut {
    const W: usize = REGION_PX;
    let mut base = vec![0u8; W * W * 3];
    let mut height = vec![NO_HEIGHT; W * W];
    let mut wdepth = vec![0u8; W * W];
    let mut wrgb = vec![0u8; W * W * 3];
    let mut cache = Cache::default();
    let default_biome = cache.biome("minecraft:plains");
    let (mut ok, mut bad) = (0u32, 0u32);
    let mut first_error: Option<String> = None;

    for cz in 0..32usize {
        for cx in 0..32usize {
            let bytes = match region.chunk_nbt(cx, cz) {
                Ok(Some(b)) => b,
                Ok(None) => continue,
                Err(e) => {
                    bad += 1;
                    first_error.get_or_insert(e);
                    continue;
                }
            };
            let chunk = match anvil::parse_chunk(&bytes) {
                Ok(Some(c)) => c,
                Ok(None) => continue,
                Err(e) => {
                    bad += 1;
                    first_error.get_or_insert(e);
                    continue;
                }
            };
            ok += 1;

            let mut secs: Vec<Sec> = chunk
                .sections
                .iter()
                .map(|s| {
                    let classes: Vec<Class> = s.palette.iter().map(|n| cache.class(n)).collect();
                    let trivial = classes.iter().all(|c| *c == Class::Skip);
                    let biome_cols: Vec<BiomeColors> = s.biomes.iter().map(|n| cache.biome(n)).collect();
                    Sec {
                        y: s.y,
                        s,
                        classes,
                        trivial,
                        bits: anvil::block_bits(s.palette.len()),
                        idx: None,
                        biome_cols,
                        bbits: anvil::biome_bits(s.biomes.len()),
                    }
                })
                .collect();

            for lz in 0..16usize {
                for lx in 0..16usize {
                    if let Some(f) = scan_column(&mut secs, lx, lz, nether, default_biome) {
                        let i = (cz * 16 + lz) * W + cx * 16 + lx;
                        height[i] = f.y.clamp(-32000, 32000) as i16;
                        base[i * 3..i * 3 + 3].copy_from_slice(&f.rgb);
                        wdepth[i] = f.water_depth;
                        wrgb[i * 3..i * 3 + 3].copy_from_slice(&f.water_rgb);
                    }
                }
            }
        }
    }

    let (rgba, has_pixels) = shade(&base, &height, &wdepth, &wrgb);
    RegionOut { rgba, has_pixels, chunks_ok: ok, chunks_bad: bad, first_error, unknown: cache.unknown }
}

fn shade(base: &[u8], height: &[i16], wdepth: &[u8], wrgb: &[u8]) -> (Vec<u8>, bool) {
    const W: usize = REGION_PX;
    let mut out = vec![0u8; W * W * 4];
    let mut any = false;
    for z in 0..W {
        for x in 0..W {
            let i = z * W + x;
            if height[i] == NO_HEIGHT {
                continue;
            }
            any = true;
            let h = height[i] as i32;
            let hn = if z > 0 && height[i - W] != NO_HEIGHT { height[i - W] as i32 } else { h };
            let hw = if x > 0 && height[i - 1] != NO_HEIGHT { height[i - 1] as i32 } else { h };
            let under_water = wdepth[i] > 0;

            // A column higher than its northern / western neighbour faces the light.
            let mut d = (h - hn).clamp(-6, 6) as f32 * 0.07 + (h - hw).clamp(-6, 6) as f32 * 0.035;
            if under_water {
                d *= 0.5;
            }
            let shade = (1.0 + d).clamp(0.62, 1.35);

            let mut c = [base[i * 3] as f32 * shade, base[i * 3 + 1] as f32 * shade, base[i * 3 + 2] as f32 * shade];
            if under_water {
                let a = (0.50 + 0.035 * (wdepth[i] as f32).min(12.0)).min(0.92);
                for k in 0..3 {
                    c[k] = c[k] * (1.0 - a) + wrgb[i * 3 + k] as f32 * a;
                }
            }
            let o = i * 4;
            out[o] = c[0].clamp(0.0, 255.0) as u8;
            out[o + 1] = c[1].clamp(0.0, 255.0) as u8;
            out[o + 2] = c[2].clamp(0.0, 255.0) as u8;
            out[o + 3] = 255;
        }
    }
    (out, any)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture;

    const GRASS: Rgb = [0x91, 0xBD, 0x59];

    fn px(out: &RegionOut, x: usize, z: usize) -> [u8; 4] {
        let o = (z * REGION_PX + x) * 4;
        [out.rgba[o], out.rgba[o + 1], out.rgba[o + 2], out.rgba[o + 3]]
    }

    fn region_with(chunks: Vec<(usize, usize, Vec<u8>)>) -> RegionFile {
        RegionFile::from_bytes(fixture::region_bytes(&chunks))
    }

    #[test]
    fn flat_grass_chunk() {
        let nbt = fixture::chunk_nbt(-4..20, &|_x, y, _z| if y > 64 { "minecraft:air" } else if y == 64 { "minecraft:grass_block" } else { "minecraft:stone" }, "minecraft:plains");
        let out = render_region(&region_with(vec![(0, 0, nbt)]), false);
        assert_eq!(out.chunks_ok, 1);
        assert_eq!(out.chunks_bad, 0);
        assert!(out.has_pixels);
        assert_eq!(px(&out, 3, 3), [GRASS[0], GRASS[1], GRASS[2], 255]);
        assert_eq!(px(&out, 15, 15)[3], 255);
        assert_eq!(px(&out, 16, 0)[3], 0, "outside the chunk is transparent");
        assert_eq!(px(&out, 0, 16)[3], 0);
    }

    #[test]
    fn slopes_are_shaded() {
        // rises towards +x: each column is higher than its western neighbour -> brighter
        let up = fixture::chunk_nbt(-4..20, &|x, y, _z| if y > 64 + x as i32 { "minecraft:air" } else if y == 64 + x as i32 { "minecraft:grass_block" } else { "minecraft:stone" }, "minecraft:plains");
        let out = render_region(&region_with(vec![(0, 0, up)]), false);
        let p = px(&out, 8, 4);
        assert!(p[1] > GRASS[1], "{p:?}");
        // falls towards +x: darker
        let down = fixture::chunk_nbt(-4..20, &|x, y, _z| if y > 80 - x as i32 { "minecraft:air" } else if y == 80 - x as i32 { "minecraft:grass_block" } else { "minecraft:stone" }, "minecraft:plains");
        let out = render_region(&region_with(vec![(0, 0, down)]), false);
        let p = px(&out, 8, 4);
        assert!(p[1] < GRASS[1], "{p:?}");
    }

    #[test]
    fn plants_and_glass_are_looked_through() {
        let nbt = fixture::chunk_nbt(-4..20, &|_x, y, _z| match y {
            70 => "minecraft:glass",
            69 => "minecraft:short_grass",
            68 => "minecraft:oak_sign",
            64 => "minecraft:sand",
            y if y < 64 => "minecraft:stone",
            _ => "minecraft:air",
        }, "minecraft:desert");
        let out = render_region(&region_with(vec![(0, 0, nbt)]), false);
        assert_eq!(px(&out, 2, 2), [219, 207, 163, 255]);
    }

    #[test]
    fn water_is_blended_by_depth() {
        let shallow = fixture::chunk_nbt(-4..20, &|_x, y, _z| if y > 64 { "minecraft:air" } else if y > 62 { "minecraft:water" } else if y == 62 { "minecraft:sand" } else { "minecraft:stone" }, "minecraft:plains");
        let deep = fixture::chunk_nbt(-4..20, &|_x, y, _z| if y > 64 { "minecraft:air" } else if y > 40 { "minecraft:water" } else if y == 40 { "minecraft:sand" } else { "minecraft:stone" }, "minecraft:plains");
        let a = px(&render_region(&region_with(vec![(0, 0, shallow)]), false), 5, 5);
        let b = px(&render_region(&region_with(vec![(0, 0, deep)]), false), 5, 5);
        assert!(a[2] > a[0], "water looks bluish: {a:?}");
        assert!(b[2] > b[0], "{b:?}");
        assert!(a[0] > b[0], "shallow water shows more of the sand: {a:?} vs {b:?}");
    }

    #[test]
    fn nether_uses_the_cave_not_the_roof() {
        let nbt = fixture::chunk_nbt(0..8, &|_x, y, _z| if y >= 123 { "minecraft:bedrock" } else if (40..=60).contains(&y) { "minecraft:air" } else { "minecraft:netherrack" }, "minecraft:nether_wastes");
        let out = render_region(&region_with(vec![(0, 0, nbt.clone())]), true);
        assert_eq!(px(&out, 4, 4), [98, 38, 38, 255], "netherrack floor, not the grey bedrock roof");
        let as_overworld = render_region(&region_with(vec![(0, 0, nbt)]), false);
        assert_eq!(px(&as_overworld, 4, 4), [85, 85, 85, 255]);
    }

    #[test]
    fn unknown_blocks_are_reported_and_grey() {
        let nbt = fixture::chunk_nbt(-4..20, &|_x, y, _z| if y == 10 { "somemod:weird_block" } else if y < 10 { "minecraft:stone" } else { "minecraft:air" }, "minecraft:plains");
        let out = render_region(&region_with(vec![(0, 0, nbt)]), false);
        assert_eq!(px(&out, 1, 1), [128, 128, 128, 255]);
        assert!(out.unknown.contains("somemod:weird_block"));
    }

    #[test]
    fn bad_chunk_is_counted_not_fatal() {
        let good = fixture::chunk_nbt(-4..20, &|_x, y, _z| if y == 64 { "minecraft:sand" } else if y < 64 { "minecraft:stone" } else { "minecraft:air" }, "minecraft:plains");
        let out = render_region(&region_with(vec![(0, 0, good), (1, 0, vec![1, 2, 3])]), false);
        assert_eq!(out.chunks_ok, 1);
        assert_eq!(out.chunks_bad, 1);
        assert!(out.first_error.is_some());
        assert!(out.has_pixels);
    }

    #[test]
    fn empty_region_has_no_pixels() {
        let out = render_region(&RegionFile::from_bytes(Vec::new()), false);
        assert!(!out.has_pixels);
        let out = render_region(&region_with(vec![]), false);
        assert!(!out.has_pixels);
    }
}
