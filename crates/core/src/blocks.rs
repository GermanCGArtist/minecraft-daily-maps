//! How a block (and a biome) looks from above.
//!
//! The colours are approximations of the block textures, not exact values. They are all in
//! this file, so a colour that looks wrong is a one-line change. Blocks this file does not
//! know are drawn grey and reported in the log, so the table can grow with the world.
//! If you change colours, also bump `RENDERER_ID` in `native.rs`, otherwise old tiles that
//! were copied from the previous snapshot keep the old look.

pub type Rgb = [u8; 3];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tint {
    None,
    /// Use the biome's grass colour.
    Grass,
    /// Use the biome's leaf colour.
    Foliage,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Class {
    /// Not visible from above (air, plants, glass, torches, ...): look through it.
    Skip,
    Water,
    Solid { rgb: Rgb, tint: Tint },
}

fn solid(r: u8, g: u8, b: u8) -> Class {
    Class::Solid { rgb: [r, g, b], tint: Tint::None }
}

fn tinted(tint: Tint) -> Class {
    Class::Solid { rgb: [255, 255, 255], tint }
}

const GREY: Rgb = [128, 128, 128];

/// `(class, known)`. `known == false` means the name matched no rule and is drawn grey.
pub fn classify(full_name: &str) -> (Class, bool) {
    let n = full_name.strip_prefix("minecraft:").unwrap_or(full_name);
    match lookup(n) {
        Some(c) => (c, true),
        None => (Class::Solid { rgb: GREY, tint: Tint::None }, false),
    }
}

fn is_skipped(n: &str) -> bool {
    matches!(
        n,
        "air" | "cave_air" | "void_air" | "light" | "barrier" | "structure_void" | "structure_block" | "jigsaw"
            | "moving_piston" | "piston_head" | "glass" | "tinted_glass"
            | "short_grass" | "grass" | "tall_grass" | "fern" | "large_fern" | "dead_bush" | "bush" | "firefly_bush"
            | "short_dry_grass" | "tall_dry_grass" | "leaf_litter" | "wildflowers" | "cactus_flower"
            | "dandelion" | "poppy" | "blue_orchid" | "allium" | "azure_bluet" | "oxeye_daisy" | "cornflower"
            | "lily_of_the_valley" | "wither_rose" | "torchflower" | "pink_petals" | "sunflower" | "lilac"
            | "rose_bush" | "peony" | "pitcher_plant" | "open_eyeblossom" | "closed_eyeblossom"
            | "spore_blossom" | "sweet_berry_bush" | "vine" | "cave_vines" | "cave_vines_plant" | "glow_lichen"
            | "sculk_vein" | "hanging_roots" | "lily_pad" | "sugar_cane" | "bamboo" | "bamboo_sapling"
            | "sea_pickle" | "torch" | "wall_torch" | "soul_torch" | "soul_wall_torch" | "redstone_torch"
            | "redstone_wall_torch" | "copper_torch" | "copper_wall_torch" | "lantern" | "soul_lantern"
            | "end_rod" | "lightning_rod" | "chain" | "iron_bars" | "ladder" | "lever" | "tripwire"
            | "tripwire_hook" | "redstone_wire" | "string" | "cobweb" | "fire" | "soul_fire" | "rail"
            | "powered_rail" | "detector_rail" | "activator_rail" | "frogspawn" | "turtle_egg" | "sniffer_egg"
            | "flower_pot" | "nether_sprouts" | "crimson_roots" | "warped_roots" | "crimson_fungus"
            | "warped_fungus" | "weeping_vines" | "weeping_vines_plant" | "twisting_vines"
            | "twisting_vines_plant" | "small_dripleaf" | "big_dripleaf" | "big_dripleaf_stem"
            | "pointed_dripstone" | "amethyst_cluster" | "large_amethyst_bud" | "medium_amethyst_bud"
            | "small_amethyst_bud" | "candle" | "red_mushroom" | "brown_mushroom" | "azalea" | "flowering_azalea"
            | "wheat" | "carrots" | "potatoes" | "beetroots" | "melon_stem" | "pumpkin_stem"
            | "attached_melon_stem" | "attached_pumpkin_stem" | "torchflower_crop" | "pitcher_crop" | "cocoa"
            | "nether_wart" | "end_portal" | "end_gateway" | "nether_portal" | "bubble_coral_fan"
            | "brain_coral_fan" | "fire_coral_fan" | "horn_coral_fan" | "tube_coral_fan"
    ) || n.ends_with("_sign")
        || n.ends_with("_banner")
        || n.ends_with("_head")
        || n.ends_with("_skull")
        || n.ends_with("_button")
        || n.ends_with("_pressure_plate")
        || n.ends_with("_pane")
        || n.ends_with("_glass")
        || n.ends_with("_sapling")
        || n.ends_with("_tulip")
        || n.ends_with("_candle")
        || n.ends_with("_coral_wall_fan")
        || n.ends_with("_coral_fan")
        || n.starts_with("potted_")
}

const DYES: [(&str, Rgb); 16] = [
    ("light_blue", [58, 175, 217]),
    ("light_gray", [142, 142, 134]),
    ("white", [233, 236, 236]),
    ("orange", [240, 118, 19]),
    ("magenta", [189, 68, 179]),
    ("yellow", [248, 197, 39]),
    ("lime", [112, 185, 25]),
    ("pink", [237, 141, 172]),
    ("gray", [62, 68, 71]),
    ("cyan", [21, 137, 145]),
    ("purple", [121, 42, 172]),
    ("blue", [53, 57, 157]),
    ("brown", [114, 71, 40]),
    ("green", [84, 109, 27]),
    ("red", [160, 39, 34]),
    ("black", [20, 21, 25]),
];

const TERRACOTTA: [(&str, Rgb); 16] = [
    ("light_blue", [113, 108, 137]),
    ("light_gray", [135, 106, 97]),
    ("white", [209, 178, 161]),
    ("orange", [161, 83, 37]),
    ("magenta", [149, 88, 108]),
    ("yellow", [186, 133, 35]),
    ("lime", [103, 117, 52]),
    ("pink", [161, 78, 78]),
    ("gray", [57, 42, 35]),
    ("cyan", [87, 91, 91]),
    ("purple", [118, 70, 86]),
    ("blue", [74, 60, 91]),
    ("brown", [77, 51, 36]),
    ("green", [76, 83, 42]),
    ("red", [143, 61, 47]),
    ("black", [37, 23, 16]),
];

/// `(name, planks colour)`. Order matters: longer names first.
const WOODS: [(&str, Rgb); 13] = [
    ("dark_oak", [66, 43, 20]),
    ("pale_oak", [228, 220, 214]),
    ("bamboo", [193, 173, 81]),
    ("mangrove", [117, 54, 48]),
    ("cherry", [226, 178, 172]),
    ("spruce", [114, 84, 48]),
    ("birch", [192, 175, 121]),
    ("jungle", [160, 115, 80]),
    ("acacia", [168, 90, 50]),
    ("crimson", [101, 48, 70]),
    ("warped", [43, 104, 99]),
    ("oak", [162, 130, 78]),
    ("petrified", [162, 130, 78]),
];

fn scale(c: Rgb, f: f32) -> Rgb {
    [(c[0] as f32 * f).min(255.0) as u8, (c[1] as f32 * f).min(255.0) as u8, (c[2] as f32 * f).min(255.0) as u8]
}

fn strip_shape(n: &str) -> Option<&str> {
    for suf in ["_slab", "_stairs", "_wall", "_fence_gate", "_fence", "_door", "_trapdoor", "_pressure_plate"] {
        if let Some(b) = n.strip_suffix(suf) {
            return Some(b);
        }
    }
    None
}

fn lookup(n: &str) -> Option<Class> {
    if is_skipped(n) {
        return Some(Class::Skip);
    }
    match n {
        "water" | "bubble_column" | "kelp" | "kelp_plant" | "seagrass" | "tall_seagrass" => return Some(Class::Water),
        "lava" => return Some(solid(222, 92, 18)),
        _ => {}
    }

    // Leaves
    if let Some(kind) = n.strip_suffix("_leaves") {
        return Some(match kind {
            "spruce" => solid(60, 98, 60),
            "birch" => solid(108, 146, 70),
            "cherry" => solid(232, 160, 195),
            "azalea" => solid(96, 128, 48),
            "flowering_azalea" => solid(112, 126, 72),
            "pale_oak" => solid(170, 188, 160),
            _ => tinted(Tint::Foliage),
        });
    }

    // Terrain and natural blocks
    let c = match n {
        "grass_block" => tinted(Tint::Grass),
        "mycelium" => solid(111, 99, 105),
        "podzol" => solid(94, 66, 28),
        "dirt" => solid(134, 96, 67),
        "coarse_dirt" => solid(119, 85, 59),
        "rooted_dirt" => solid(144, 103, 76),
        "farmland" => solid(110, 76, 45),
        "dirt_path" | "grass_path" => solid(148, 122, 65),
        "mud" => solid(60, 57, 60),
        "muddy_mangrove_roots" => solid(68, 60, 40),
        "mangrove_roots" => solid(110, 86, 52),
        "clay" => solid(160, 166, 179),
        "gravel" | "suspicious_gravel" => solid(131, 127, 126),
        "sand" | "suspicious_sand" => solid(219, 207, 163),
        "red_sand" => solid(190, 102, 33),
        "snow" | "snow_block" | "powder_snow" => solid(248, 253, 253),
        "ice" | "frosted_ice" => solid(145, 183, 253),
        "packed_ice" => solid(141, 180, 250),
        "blue_ice" => solid(116, 167, 253),
        "bedrock" => solid(85, 85, 85),
        "obsidian" => solid(15, 11, 25),
        "crying_obsidian" => solid(32, 10, 60),
        "netherrack" => solid(98, 38, 38),
        "soul_sand" => solid(81, 62, 50),
        "soul_soil" => solid(75, 57, 46),
        "magma_block" => solid(142, 63, 31),
        "nether_wart_block" => solid(114, 2, 2),
        "warped_wart_block" => solid(22, 119, 121),
        "shroomlight" => solid(240, 146, 70),
        "glowstone" => solid(171, 131, 84),
        "sea_lantern" => solid(172, 199, 190),
        "crimson_nylium" => solid(130, 31, 31),
        "warped_nylium" => solid(43, 114, 101),
        "bone_block" => solid(229, 225, 207),
        "end_stone" | "end_stone_bricks" => solid(219, 222, 158),
        "ancient_debris" => solid(94, 66, 58),
        "netherite_block" => solid(66, 61, 63),
        "coal_block" => solid(16, 16, 16),
        "gold_block" => solid(250, 238, 77),
        "iron_block" => solid(220, 220, 220),
        "diamond_block" => solid(92, 219, 213),
        "emerald_block" => solid(42, 203, 87),
        "lapis_block" => solid(31, 67, 140),
        "redstone_block" => solid(170, 20, 10),
        "raw_iron_block" => solid(166, 135, 107),
        "raw_copper_block" => solid(154, 105, 79),
        "raw_gold_block" => solid(221, 169, 46),
        "amethyst_block" | "budding_amethyst" => solid(133, 97, 191),
        "hay_block" => solid(166, 139, 12),
        "melon" => solid(111, 145, 30),
        "pumpkin" | "carved_pumpkin" | "jack_o_lantern" => solid(198, 118, 24),
        "slime_block" => solid(111, 192, 91),
        "honey_block" => solid(251, 185, 52),
        "honeycomb_block" => solid(229, 148, 29),
        "sponge" => solid(195, 192, 74),
        "wet_sponge" => solid(171, 181, 70),
        "moss_block" | "moss_carpet" => solid(89, 109, 45),
        "pale_moss_block" | "pale_moss_carpet" => solid(108, 115, 100),
        "sculk" | "sculk_catalyst" | "sculk_shrieker" | "sculk_sensor" => solid(12, 29, 36),
        "cactus" => solid(88, 127, 50),
        "chorus_plant" | "chorus_flower" => solid(93, 57, 93),
        "red_mushroom_block" => solid(200, 46, 45),
        "brown_mushroom_block" => solid(149, 111, 81),
        "mushroom_stem" => solid(203, 196, 185),
        "calcite" => solid(223, 224, 220),
        "tuff" => solid(108, 109, 102),
        "dripstone_block" => solid(134, 107, 92),
        "bricks" => solid(150, 97, 83),
        "mud_bricks" => solid(137, 104, 79),
        "terracotta" => solid(152, 94, 67),
        "lodestone" => solid(147, 149, 152),
        "redstone_lamp" => solid(130, 85, 45),
        "bookshelf" | "chiseled_bookshelf" => solid(162, 130, 78),
        "tnt" => solid(200, 60, 40),
        "target" => solid(225, 190, 175),
        "scaffolding" => solid(193, 173, 81),
        "cobweb_block" => solid(220, 220, 220),
        "resin_block" | "resin_bricks" | "resin_clump" => solid(220, 100, 20),
        "gilded_blackstone" => solid(55, 42, 38),
        _ => Class::Skip,
    };
    if c != Class::Skip {
        return Some(c);
    }

    // Coloured blocks
    for (dye, rgb) in DYES {
        if let Some(rest) = n.strip_prefix(dye).and_then(|r| r.strip_prefix('_')) {
            match rest {
                "terracotta" => {
                    if let Some((_, c)) = TERRACOTTA.iter().find(|(d, _)| *d == dye) {
                        return Some(Class::Solid { rgb: *c, tint: Tint::None });
                    }
                }
                "wool" | "concrete" | "concrete_powder" | "carpet" | "bed" | "shulker_box" | "glazed_terracotta" => {
                    return Some(Class::Solid { rgb, tint: Tint::None });
                }
                _ => {}
            }
        }
    }
    if n == "shulker_box" {
        return Some(solid(139, 96, 139));
    }

    // Copper family (check ore first)
    if n.ends_with("_ore") {
        if n.contains("deepslate") {
            return Some(solid(80, 80, 84));
        }
        if n.starts_with("nether_") {
            return Some(solid(98, 38, 38));
        }
        return Some(solid(125, 125, 125));
    }
    if n.contains("copper") {
        let n = n.strip_prefix("waxed_").unwrap_or(n);
        return Some(if n.contains("oxidized") {
            solid(82, 162, 132)
        } else if n.contains("weathered") {
            solid(108, 153, 110)
        } else if n.contains("exposed") {
            solid(161, 125, 103)
        } else {
            solid(192, 107, 79)
        });
    }

    // Stone families
    if n.contains("deepslate") {
        return Some(solid(80, 80, 84));
    }
    if n.contains("blackstone") {
        return Some(solid(42, 36, 41));
    }
    if n.contains("basalt") {
        return Some(solid(76, 77, 82));
    }
    if n.contains("sandstone") {
        return Some(if n.starts_with("red_") || n.contains("red_sandstone") { solid(186, 99, 29) } else { solid(216, 203, 155) });
    }
    if n.contains("end_stone") {
        return Some(solid(219, 222, 158));
    }
    if n.contains("red_nether_brick") {
        return Some(solid(70, 7, 9));
    }
    if n.contains("nether_brick") {
        return Some(solid(44, 21, 26));
    }
    if n.contains("quartz") {
        return Some(solid(235, 229, 222));
    }
    if n.contains("purpur") {
        return Some(solid(169, 125, 169));
    }
    if n.contains("prismarine") {
        return Some(if n.contains("dark") { solid(52, 92, 76) } else { solid(99, 171, 158) });
    }
    if n.contains("mud_brick") {
        return Some(solid(137, 104, 79));
    }
    if n.contains("tuff") {
        return Some(solid(108, 109, 102));
    }
    if n.contains("brick") && !n.contains("stone") {
        return Some(solid(150, 97, 83));
    }
    if n.contains("granite") {
        return Some(solid(149, 103, 85));
    }
    if n.contains("diorite") {
        return Some(solid(188, 188, 188));
    }
    if n.contains("andesite") {
        return Some(solid(136, 136, 136));
    }
    if n.contains("cobblestone") || n.contains("stone") || n == "smooth_stone" {
        return Some(solid(125, 125, 125));
    }

    // Wood family: planks, logs, stems and everything made of them
    for (w, planks) in WOODS {
        if n.contains(w) {
            let log = n.ends_with("_log") || n.ends_with("_wood") || n.ends_with("_stem") || n.ends_with("_hyphae") || n.ends_with("_block");
            return Some(Class::Solid { rgb: if log { scale(planks, 0.78) } else { planks }, tint: Tint::None });
        }
    }

    // Shapes of something we did not match directly (e.g. "xyz_slab")
    if let Some(base) = strip_shape(n) {
        if let Some(c) = lookup(base).or_else(|| lookup(&format!("{base}s"))).or_else(|| lookup(&format!("{base}_block"))) {
            return Some(c);
        }
    }

    // Containers and machines that are just "some solid block"
    if matches!(
        n,
        "chest" | "trapped_chest" | "ender_chest" | "barrel" | "crafting_table" | "furnace" | "blast_furnace" | "smoker"
            | "dispenser" | "dropper" | "piston" | "sticky_piston" | "observer" | "hopper" | "cauldron" | "water_cauldron"
            | "lava_cauldron" | "anvil" | "chipped_anvil" | "damaged_anvil" | "enchanting_table" | "brewing_stand"
            | "composter" | "beehive" | "bee_nest" | "note_block" | "jukebox" | "loom" | "grindstone" | "stonecutter"
            | "cartography_table" | "fletching_table" | "smithing_table" | "lectern" | "campfire" | "soul_campfire"
            | "bell" | "conduit" | "beacon" | "spawner" | "command_block" | "repeating_command_block"
            | "chain_command_block" | "daylight_detector" | "comparator" | "repeater" | "decorated_pot"
    ) {
        return Some(solid(110, 100, 90));
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BiomeColors {
    pub grass: Rgb,
    pub foliage: Rgb,
    pub water: Rgb,
}

fn hex(v: u32) -> Rgb {
    [(v >> 16) as u8, (v >> 8) as u8, v as u8]
}

/// Colours the game would use for grass, leaves and water in this biome (the default value
/// of each biome; Minecraft's smooth temperature/humidity gradients are not modelled).
pub fn biome_colors(full_name: &str) -> BiomeColors {
    let n = full_name.strip_prefix("minecraft:").unwrap_or(full_name);

    let water = if n.contains("lukewarm") {
        0x45ADF2
    } else if n.contains("warm_ocean") {
        0x43D5EE
    } else if n.contains("frozen") {
        0x3938C9
    } else if n.contains("cold") || n.contains("snowy") || n.contains("taiga") {
        0x3D57D6
    } else if n.contains("mangrove_swamp") {
        0x3A7A6A
    } else if n.contains("swamp") {
        0x617B64
    } else if n.contains("meadow") {
        0x0E4ECF
    } else if n.contains("cherry") {
        0x5DB7EF
    } else {
        0x3F76E4
    };

    let (grass, foliage) = if n.contains("pale_garden") {
        (0x778272, 0x878D76)
    } else if n.contains("mushroom") {
        (0x55C93F, 0x2BBB0F)
    } else if n.contains("mangrove_swamp") {
        (0x8DB127, 0x8DB127)
    } else if n.contains("swamp") {
        (0x6A7039, 0x6A7039)
    } else if n.contains("badlands") || n.contains("mesa") {
        (0x90814D, 0x9E814D)
    } else if n.contains("desert") || n.contains("savanna") {
        (0xBFB755, 0xAEA42A)
    } else if n.contains("jungle") || n.contains("bamboo") {
        (0x59C93C, 0x30BB0B)
    } else if n.contains("dark_forest") {
        (0x507A32, 0x59AE30)
    } else if n.contains("cherry") {
        (0xB6DB61, 0xB6DB61)
    } else if n.contains("meadow") {
        (0x83BB6D, 0x63A948)
    } else if n.contains("snowy") || n.contains("frozen") || n.contains("ice_spikes") || n == "grove" || n.contains("jagged") {
        (0x80B497, 0x60A17B)
    } else if n.contains("birch") {
        (0x88BB67, 0x6BA941)
    } else if n.contains("taiga") || n.contains("old_growth") {
        (0x86B783, 0x68A464)
    } else if n.contains("windswept") || n.contains("stony") || n.contains("extreme_hills") || n.contains("hills") {
        (0x8AB689, 0x6DA36B)
    } else if n.contains("forest") {
        (0x79C05A, 0x59AE30)
    } else {
        (0x91BD59, 0x77AB2F)
    };

    BiomeColors { grass: hex(grass), foliage: hex(foliage), water: hex(water) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_skip(n: &str) -> bool {
        classify(n).0 == Class::Skip
    }

    #[test]
    fn common_blocks() {
        assert!(is_skip("minecraft:air"));
        assert!(is_skip("minecraft:short_grass"));
        assert!(is_skip("minecraft:oak_sign"));
        assert!(is_skip("minecraft:potted_poppy"));
        assert_eq!(classify("minecraft:water").0, Class::Water);
        assert_eq!(classify("minecraft:kelp").0, Class::Water);
        assert_eq!(classify("minecraft:grass_block").0, tinted(Tint::Grass));
        assert_eq!(classify("minecraft:oak_leaves").0, tinted(Tint::Foliage));
        assert_eq!(classify("minecraft:sand"), (solid(219, 207, 163), true));
        assert_eq!(classify("minecraft:stone").0, solid(125, 125, 125));
        assert_eq!(classify("minecraft:stone_brick_stairs").0, solid(125, 125, 125));
        assert_eq!(classify("minecraft:iron_ore").0, solid(125, 125, 125));
        assert_eq!(classify("minecraft:deepslate_gold_ore").0, solid(80, 80, 84));
        assert_eq!(classify("minecraft:oak_planks").0, solid(162, 130, 78));
        assert_eq!(classify("minecraft:dark_oak_slab").0, solid(66, 43, 20));
        assert_eq!(classify("minecraft:red_wool").0, solid(160, 39, 34));
        assert_eq!(classify("minecraft:light_blue_terracotta").0, solid(113, 108, 137));
        assert_eq!(classify("minecraft:waxed_oxidized_cut_copper_stairs").0, solid(82, 162, 132));
        assert_eq!(classify("minecraft:sandstone_slab").0, solid(216, 203, 155));
        assert_eq!(classify("minecraft:netherrack").0, solid(98, 38, 38));
    }

    #[test]
    fn unknown_is_grey_and_flagged() {
        let (c, known) = classify("somemod:weird_block");
        assert!(!known);
        assert_eq!(c, solid(128, 128, 128));
    }

    #[test]
    fn biomes() {
        assert_eq!(biome_colors("minecraft:plains").grass, hex(0x91BD59));
        assert_eq!(biome_colors("minecraft:swamp").water, hex(0x617B64));
        assert_eq!(biome_colors("minecraft:snowy_taiga").grass, hex(0x80B497));
        assert_eq!(biome_colors("minecraft:old_growth_birch_forest").grass, hex(0x88BB67));
        assert_eq!(biome_colors("minecraft:windswept_savanna").grass, hex(0xBFB755));
        assert_eq!(biome_colors("minecraft:deep_ocean").water, hex(0x3F76E4));
    }
}
