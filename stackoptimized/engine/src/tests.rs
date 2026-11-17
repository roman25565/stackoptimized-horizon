use super::*;
use serde_json::json;

fn comps(size: i64) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert(STACK_SIZE.into(), size.into());
    m
}

fn damageable() -> Map<String, Value> {
    let mut m = comps(1);
    m.insert(MAX_DAMAGE.into(), 250.into());
    m
}

fn rules(text: &str) -> Rules {
    let r = Rules::parse(text);
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    r
}

#[test]
fn default_config_parses_and_changes_nothing() {
    let r = rules(DEFAULT_CONFIG);
    assert!(r.groups.is_empty() && r.items.is_empty());
    assert!(!r.allow_lowering);
    for item in ["minecraft:potion", "minecraft:stone", "minecraft:white_bed"] {
        assert_eq!(decide(&r, item, &comps(1), None).set, None);
    }
}

#[test]
fn groups_match_their_items_only() {
    let yes = [
        (Group::ShulkerBoxes, "minecraft:shulker_box"),
        (Group::ShulkerBoxes, "minecraft:light_blue_shulker_box"),
        (Group::Beds, "minecraft:red_bed"),
        (Group::Potions, "minecraft:lingering_potion"),
        (Group::Minecarts, "minecraft:minecart"),
        (Group::Minecarts, "minecraft:hopper_minecart"),
        (Group::Boats, "minecraft:pale_oak_chest_boat"),
        (Group::Boats, "minecraft:bamboo_raft"),
        (Group::Banners, "minecraft:black_banner"),
        (Group::MusicDiscs, "minecraft:music_disc_lava_chicken"),
        (Group::Buckets, "minecraft:bucket"),
        (Group::EnchantedBooks, "minecraft:enchanted_book"),
    ];
    for (g, item) in yes {
        assert!(g.contains(item), "{g:?} {item}");
    }
    let no = [
        (Group::Beds, "minecraft:bedrock"),
        (Group::Banners, "minecraft:banner_pattern"),
        (Group::Buckets, "minecraft:water_bucket"),
        (Group::MusicDiscs, "minecraft:disc_fragment_5"),
        (Group::Potions, "minecraft:potion_extra:x"),
        (Group::ShulkerBoxes, "othermod:white_shulker_box"),
    ];
    for (g, item) in no {
        assert!(!g.contains(item), "{g:?} {item}");
    }
}

#[test]
fn old_plugin_names_still_work() {
    let r = rules(
        "[groups]\nSkulkers = 64\nMusic_Disc = \"16\"\nBeds = \"default\"\nBoats = false\n\
         [items]\nSNOWBALL = 64\nender_pearl = 32\n\"minecraft:egg\" = 64\n",
    );
    assert_eq!(r.groups.get(&Group::ShulkerBoxes), Some(&64));
    assert_eq!(r.groups.get(&Group::MusicDiscs), Some(&16));
    assert!(!r.groups.contains_key(&Group::Beds));
    assert!(!r.groups.contains_key(&Group::Boats));
    assert_eq!(r.items.get("minecraft:snowball"), Some(&64));
    assert_eq!(r.items.get("minecraft:ender_pearl"), Some(&32));
    assert_eq!(r.items.get("minecraft:egg"), Some(&64));
}

#[test]
fn bad_values_are_skipped_with_a_warning() {
    let r = Rules::parse(
        "[groups]\nbeds = 0\npotions = 100\nnope = 5\n[items]\nstone = \"lots\"\n\
         snowball = 64\n[extra]\n[settings]\nprint_table = 3\n",
    );
    assert!(r.groups.is_empty());
    assert_eq!(r.items.len(), 1);
    assert_eq!(r.warnings.len(), 6, "{:?}", r.warnings);
}

#[test]
fn broken_toml_changes_nothing() {
    let r = Rules::parse("[groups\nbeds = 16");
    assert_eq!(r.warnings.len(), 1);
    assert_eq!(decide(&r, "minecraft:red_bed", &comps(1), None).set, None);
}

#[test]
fn raises_and_remembers() {
    let r = rules("[groups]\npotions = 16\n[items]\nsnowball = 64\n");
    let d = decide(&r, "minecraft:potion", &comps(1), None);
    assert_eq!((d.set, d.remember, d.note), (Some(16), Some(16), None));
    let d = decide(&r, "minecraft:snowball", &comps(16), Some(64));
    assert_eq!((d.set, d.remember), (Some(64), None));
    assert_eq!(decide(&r, "minecraft:stone", &comps(64), None).set, None);
}

#[test]
fn item_entry_beats_its_group() {
    let r = rules("[groups]\nbeds = 16\n[items]\nred_bed = 8\n");
    assert_eq!(
        decide(&r, "minecraft:red_bed", &comps(1), None).set,
        Some(8)
    );
    assert_eq!(
        decide(&r, "minecraft:blue_bed", &comps(1), None).set,
        Some(16)
    );
}

#[test]
fn unsafe_items_are_never_raised() {
    let r = rules(
        "[groups]\nbuckets = 16\nenchanted_books = 16\n[items]\nwater_bucket = 16\n\
         lava_bucket = 16\naxolotl_bucket = 16\nmilk_bucket = 16\npowder_snow_bucket = 16\n\
         bundle = 16\nred_bundle = 16\ndiamond_sword = 16\n",
    );
    for item in [
        "minecraft:water_bucket",
        "minecraft:lava_bucket",
        "minecraft:axolotl_bucket",
        "minecraft:milk_bucket",
        "minecraft:powder_snow_bucket",
        "minecraft:bundle",
        "minecraft:red_bundle",
        "minecraft:enchanted_book",
    ] {
        let d = decide(&r, item, &comps(1), None);
        assert_eq!((d.set, d.remember), (None, None), "{item}");
        assert!(d.note.is_some(), "{item}");
    }
    // Durability: refused even if the ledger (from some older version) says otherwise.
    let d = decide(&r, "minecraft:diamond_sword", &damageable(), Some(16));
    assert_eq!((d.set, d.remember), (None, None));
    // The empty bucket is fine.
    assert_eq!(decide(&r, "minecraft:bucket", &comps(16), None).set, None);
    let r = rules("[groups]\nbuckets = 64\n");
    assert_eq!(
        decide(&r, "minecraft:bucket", &comps(16), None).set,
        Some(64)
    );
}

#[test]
fn never_lowers_without_permission() {
    // Below vanilla.
    let r = rules("[items]\nstone = 16\n");
    let d = decide(&r, "minecraft:stone", &comps(64), None);
    assert_eq!(d.set, None);
    assert!(d.note.is_some());
    // Below what the mod applied before: potions were 16, config now says 8.
    let r = rules("[groups]\npotions = 8\n");
    let d = decide(&r, "minecraft:potion", &comps(1), Some(16));
    assert_eq!((d.set, d.remember), (Some(16), None));
    assert!(d.note.is_some());
    // Removed from the config: still kept at 16.
    let r = rules("");
    let d = decide(&r, "minecraft:potion", &comps(1), Some(16));
    assert_eq!(d.set, Some(16));
}

#[test]
fn lowers_when_allowed() {
    let r =
        rules("[settings]\nallow_lowering = true\n[groups]\npotions = 8\n[items]\nstone = 16\n");
    let d = decide(&r, "minecraft:potion", &comps(1), Some(16));
    assert_eq!((d.set, d.remember), (Some(8), None));
    assert_eq!(
        decide(&r, "minecraft:stone", &comps(64), None).set,
        Some(16)
    );
    // Not in the config any more: back to vanilla.
    let r = rules("[settings]\nallow_lowering = true\n");
    assert_eq!(
        decide(&r, "minecraft:potion", &comps(1), Some(16)).set,
        None
    );
}

#[test]
fn no_size_in_report_is_left_alone() {
    let r = rules("[items]\nstone = 16\n");
    assert_eq!(decide(&r, "minecraft:stone", &Map::new(), None).set, None);
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("stackoptimized-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir
}

#[test]
fn writes_default_config_and_keeps_ledger_across_restarts() {
    let dir = temp_dir("ledger");
    // First start: no config, the default is written and nothing changes.
    let m = StackOptimized::from_config_dir(&dir);
    assert_eq!(
        fs::read_to_string(dir.join(CONFIG_FILE)).unwrap(),
        DEFAULT_CONFIG
    );
    let mut c = comps(1);
    m.apply("minecraft:potion", &mut c);
    assert_eq!(c[STACK_SIZE], json!(1));

    // The admin raises potions; asked several times, the same answer.
    fs::write(dir.join(CONFIG_FILE), "[groups]\npotions = 16\n").unwrap();
    let m = StackOptimized::from_config_dir(&dir);
    for _ in 0..3 {
        let mut c = comps(1);
        m.apply("minecraft:potion", &mut c);
        assert_eq!(c[STACK_SIZE], json!(16));
    }
    let ledger: BTreeMap<String, i64> =
        serde_json::from_slice(&fs::read(dir.join(LEDGER_FILE)).unwrap()).unwrap();
    assert_eq!(ledger.get("minecraft:potion"), Some(&16));

    // Next restart the admin removes it: the ledger keeps the potions at 16.
    fs::write(dir.join(CONFIG_FILE), "").unwrap();
    let m = StackOptimized::from_config_dir(&dir);
    let mut c = comps(1);
    m.apply("minecraft:potion", &mut c);
    assert_eq!(c[STACK_SIZE], json!(16));
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn other_components_are_untouched() {
    let r = rules("[groups]\nbeds = 16\n");
    let m = StackOptimized::new(r, Ledger::default());
    let mut c = comps(1);
    c.insert("minecraft:rarity".into(), json!("common"));
    let before = c.clone();
    m.apply("minecraft:red_bed", &mut c);
    assert_eq!(c.len(), before.len());
    assert_eq!(c["minecraft:rarity"], before["minecraft:rarity"]);
    assert_eq!(c[STACK_SIZE], json!(16));
}

/// A mod in a temp config dir that has "read" a few items' defaults.
fn started(name: &str, config: &str) -> (StackOptimized, PathBuf) {
    let dir = temp_dir(name);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(CONFIG_FILE), config).unwrap();
    let m = StackOptimized::from_config_dir(&dir);
    for (item, size) in [
        ("minecraft:potion", 1),
        ("minecraft:stone", 64),
        ("minecraft:water_bucket", 1),
        ("minecraft:red_bed", 1),
        ("minecraft:snowball", 16),
    ] {
        m.apply(item, &mut comps(size));
    }
    m.apply("minecraft:diamond_sword", &mut damageable());
    (m, dir)
}

#[test]
fn written_config_reads_back_the_same() {
    let r = rules(
        "[settings]\nallow_lowering = true\n[groups]\npotions = 16\n[items]\n\"minecraft:snowball\" = 64\n\"othermod:thing\" = 5\n",
    );
    let again = rules(&r.to_toml());
    assert_eq!(again, r);
}

#[test]
fn set_item_writes_for_the_next_start_only() {
    let (m, dir) = started("set", "");
    let msg = m.set_item("minecraft:snowball", Some(64)).unwrap();
    assert!(
        msg.contains("now 16") && msg.contains("after a server restart 64"),
        "{msg}"
    );
    // Nothing changes while the game runs.
    let mut c = comps(16);
    m.apply("minecraft:snowball", &mut c);
    assert_eq!(c[STACK_SIZE], json!(16));
    // The next start uses it.
    let next = StackOptimized::from_config_dir(&dir);
    let mut c = comps(16);
    next.apply("minecraft:snowball", &mut c);
    assert_eq!(c[STACK_SIZE], json!(64));
    // Removing the entry.
    m.set_item("minecraft:snowball", None).unwrap();
    assert!(
        !m.pending()
            .unwrap()
            .items
            .contains_key("minecraft:snowball")
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn set_item_refuses_unsafe_and_bad_sizes() {
    let (m, dir) = started("refuse", "");
    assert!(m.set_item("minecraft:water_bucket", Some(16)).is_err());
    assert!(m.set_item("minecraft:diamond_sword", Some(2)).is_err());
    assert!(m.set_item("minecraft:stone", Some(0)).is_err());
    assert!(m.set_item("minecraft:stone", Some(100)).is_err());
    assert!(m.pending().unwrap().items.is_empty());
    // Lowering is written, but the answer says it won't take effect.
    let msg = m.set_item("minecraft:stone", Some(16)).unwrap();
    assert!(
        msg.contains("after a server restart 64") && msg.contains("allow_lowering"),
        "{msg}"
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_broken_file_is_never_overwritten() {
    let (m, dir) = started("broken", "[groups]\nbeds = 16\n");
    fs::write(dir.join(CONFIG_FILE), "[groups\nbeds = 16").unwrap();
    assert!(m.set_item("minecraft:snowball", Some(64)).is_err());
    assert_eq!(
        fs::read_to_string(dir.join(CONFIG_FILE)).unwrap(),
        "[groups\nbeds = 16"
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn menu_clicks_edit_the_file() {
    use gui::{Click, Target, click};
    let (m, dir) = started("gui", "");
    assert_eq!(click(0, 0), Click::Change(1));
    assert_eq!(click(1, 1), Click::Change(-10));
    assert_eq!(click(4, 0), Click::Remove);
    assert_eq!(click(2, 3), Click::Nothing);

    let beds = Target::Group(Group::Beds);
    // Default beds (vanilla 1): shift-left gives 11, left 12, right 11.
    m.click_cell(&beds, Click::Change(10)).unwrap();
    m.click_cell(&beds, Click::Change(1)).unwrap();
    m.click_cell(&beds, Click::Change(-1)).unwrap();
    assert_eq!(m.pending().unwrap().groups.get(&Group::Beds), Some(&11));
    // Never past 1..=99.
    for _ in 0..12 {
        m.click_cell(&beds, Click::Change(10)).unwrap();
    }
    assert_eq!(m.pending().unwrap().groups.get(&Group::Beds), Some(&99));
    m.click_cell(&beds, Click::Remove).unwrap();
    assert!(!m.pending().unwrap().groups.contains_key(&Group::Beds));
    // Books are blocked.
    assert!(
        m.click_cell(&Target::Group(Group::EnchantedBooks), Click::Change(1))
            .is_err()
    );

    // An item from the inventory, then adjusted.
    m.add_item("minecraft:snowball").unwrap();
    assert_eq!(
        m.pending().unwrap().items.get("minecraft:snowball"),
        Some(&16)
    );
    let snow = Target::Item("minecraft:snowball".into());
    m.click_cell(&snow, Click::Change(10)).unwrap();
    assert_eq!(
        m.pending().unwrap().items.get("minecraft:snowball"),
        Some(&26)
    );

    let cells = m.cells(&m.pending().unwrap());
    assert_eq!(cells.len(), Group::ALL.len() + 1);
    assert_eq!(cells.last().unwrap().count, 26);
    // Live sizes are untouched.
    assert_eq!(m.seen("minecraft:snowball").unwrap().now, 16);
    fs::remove_dir_all(&dir).unwrap();
}
