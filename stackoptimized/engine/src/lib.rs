//! StackOptimized for Horizon: sets the default `minecraft:max_stack_size` of
//! items from a config file, the way a data pack's item component would.
//!
//! Changing the item's *default* (instead of a component on each stack, as the
//! old Spigot plugin did) means every stack of the item follows it, wherever
//! it is (chests, hoppers, the ground), and nothing extra is saved in stacks:
//! taking the mod out returns every item to vanilla.
//!
//! The rules here are about never duplicating or losing items:
//!
//! * a size never goes above 99 (what a vanilla client can show and the old
//!   plugin allowed);
//! * items that break when stacked are never raised: anything with durability
//!   (`minecraft:max_damage`, the engine rejects it), filled buckets (emptying
//!   one replaces the whole held stack with a single bucket), bundles (inserting
//!   into a stack of bundles fills every bundle in it) and enchanted books (the
//!   anvil consumes the whole stack);
//! * a size is never lowered, below vanilla's or below a size this mod applied
//!   before (kept in `config/stackoptimized-applied.json`), unless the admin
//!   sets `allow_lowering = true`: containers trim a stack to the item's limit
//!   when they set it, and the rest of the stack would be gone.

use serde_json::{Map, Value};
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

pub const STACK_SIZE: &str = "minecraft:max_stack_size";
pub const MAX_DAMAGE: &str = "minecraft:max_damage";
/// The largest size the mod sets (vanilla's `ExtraCodecs.intRange(1, 99)`).
pub const MAX_SIZE: i64 = 99;
pub const CONFIG_FILE: &str = "stackoptimized.toml";
pub const LEDGER_FILE: &str = "stackoptimized-applied.json";
pub const DEFAULT_CONFIG: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/data/stackoptimized.toml"
));
const LOG: &str = "[StackOptimized]";
/// Horizon's logger keeps only targets starting with `horizon` (plus errors).
const TARGET: &str = "horizon::stackoptimized";

const COLORS: [&str; 16] = [
    "white",
    "orange",
    "magenta",
    "light_blue",
    "yellow",
    "lime",
    "pink",
    "gray",
    "light_gray",
    "cyan",
    "purple",
    "blue",
    "brown",
    "green",
    "red",
    "black",
];

/// The old plugin's groups, matched by id pattern so new items of the same
/// kind in 26.3 (a new boat, a new music disc) join their group.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    ShulkerBoxes,
    Beds,
    Potions,
    EnchantedBooks,
    Minecarts,
    Boats,
    Banners,
    MusicDiscs,
    Buckets,
}

impl Group {
    pub const ALL: [Group; 9] = [
        Group::ShulkerBoxes,
        Group::Beds,
        Group::Potions,
        Group::EnchantedBooks,
        Group::Minecarts,
        Group::Boats,
        Group::Banners,
        Group::MusicDiscs,
        Group::Buckets,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Group::ShulkerBoxes => "shulker_boxes",
            Group::Beds => "beds",
            Group::Potions => "potions",
            Group::EnchantedBooks => "enchanted_books",
            Group::Minecarts => "minecarts",
            Group::Boats => "boats",
            Group::Banners => "banners",
            Group::MusicDiscs => "music_discs",
            Group::Buckets => "buckets",
        }
    }

    /// The new name, or the old plugin's (`Skulkers`, `Music_Disc`...).
    pub fn from_name(name: &str) -> Option<Group> {
        let n = name.trim().to_ascii_lowercase();
        Some(match n.as_str() {
            "shulker_boxes" | "shulkers" | "skulkers" => Group::ShulkerBoxes,
            "beds" => Group::Beds,
            "potions" => Group::Potions,
            "enchanted_books" => Group::EnchantedBooks,
            "minecarts" => Group::Minecarts,
            "boats" => Group::Boats,
            "banners" => Group::Banners,
            "music_discs" | "music_disc" => Group::MusicDiscs,
            "buckets" => Group::Buckets,
            _ => return None,
        })
    }

    pub fn contains(self, item: &str) -> bool {
        let Some(path) = item.strip_prefix("minecraft:") else {
            return false;
        };
        let colored = |suffix: &str| {
            path.strip_suffix(suffix)
                .is_some_and(|color| COLORS.contains(&color))
        };
        match self {
            Group::ShulkerBoxes => path == "shulker_box" || colored("_shulker_box"),
            Group::Beds => colored("_bed"),
            Group::Potions => matches!(path, "potion" | "splash_potion" | "lingering_potion"),
            Group::EnchantedBooks => path == "enchanted_book",
            Group::Minecarts => path == "minecart" || path.ends_with("_minecart"),
            Group::Boats => path.ends_with("_boat") || path.ends_with("_raft"),
            Group::Banners => colored("_banner"),
            Group::MusicDiscs => path.starts_with("music_disc_"),
            // Only the empty bucket: filled ones are blocked (see `blocked`).
            Group::Buckets => path == "bucket",
        }
    }
}

/// Why an item may not be raised above its vanilla size, if it may not.
pub fn blocked(item: &str, components: &Map<String, Value>) -> Option<&'static str> {
    if components.contains_key(MAX_DAMAGE) {
        return Some("has durability: the engine refuses stackable damageable items");
    }
    let path = item.strip_prefix("minecraft:").unwrap_or(item);
    if path.ends_with("_bucket") {
        return Some("filled bucket: emptying it replaces the whole stack with one bucket");
    }
    if path == "bundle" || path.ends_with("_bundle") {
        return Some("bundle: putting items into a stack of bundles fills every bundle");
    }
    if path == "enchanted_book" {
        return Some("enchanted book: the anvil consumes the whole stack of books");
    }
    None
}

/// `SNOWBALL`, `snowball` or `minecraft:snowball` as `minecraft:snowball`.
pub fn normalize_item(name: &str) -> String {
    let name = name.trim().to_ascii_lowercase();
    if name.contains(':') {
        name
    } else {
        format!("minecraft:{name}")
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rules {
    pub print_table: bool,
    pub allow_lowering: bool,
    pub groups: BTreeMap<Group, i64>,
    pub items: BTreeMap<String, i64>,
    /// Problems found while reading the config, to log once.
    pub warnings: Vec<String>,
}

/// `16`, `"16"`, `"default"`/`false` (None), or an error.
fn parse_size(value: &toml::Value) -> Result<Option<i64>, String> {
    let n = match value {
        toml::Value::Integer(n) => *n,
        toml::Value::Boolean(false) => return Ok(None),
        toml::Value::String(s) if s.trim().eq_ignore_ascii_case("default") => return Ok(None),
        toml::Value::String(s) => s
            .trim()
            .parse::<i64>()
            .map_err(|_| format!("`{s}` is not a number or \"default\""))?,
        other => return Err(format!("`{other}` is not a number or \"default\"")),
    };
    if (1..=MAX_SIZE).contains(&n) {
        Ok(Some(n))
    } else {
        Err(format!("{n} is outside 1..={MAX_SIZE}"))
    }
}

impl Rules {
    /// Reads the config. A broken file gives rules that change nothing (the
    /// ledger still keeps sizes from shrinking), with the error as a warning.
    pub fn parse(text: &str) -> Rules {
        let mut rules = Rules {
            print_table: true,
            ..Rules::default()
        };
        let table: toml::Table = match text.parse() {
            Ok(table) => table,
            Err(e) => {
                rules.warnings.push(format!(
                    "config is not valid TOML, no stack size is changed: {e}"
                ));
                return rules;
            }
        };
        if let Some(settings) = table.get("settings").and_then(|v| v.as_table()) {
            for (key, value) in settings {
                match (key.as_str(), value.as_bool()) {
                    ("print_table", Some(b)) => rules.print_table = b,
                    ("allow_lowering", Some(b)) => rules.allow_lowering = b,
                    _ => rules
                        .warnings
                        .push(format!("settings.{key}: unknown setting or not true/false")),
                }
            }
        }
        if let Some(groups) = table.get("groups").and_then(|v| v.as_table()) {
            for (key, value) in groups {
                let Some(group) = Group::from_name(key) else {
                    rules.warnings.push(format!(
                        "groups.{key}: unknown group (known: {})",
                        Group::ALL.map(Group::name).join(", ")
                    ));
                    continue;
                };
                match parse_size(value) {
                    Ok(Some(n)) => {
                        rules.groups.insert(group, n);
                    }
                    Ok(None) => {}
                    Err(e) => rules.warnings.push(format!("groups.{key}: {e}, skipped")),
                }
            }
        }
        if let Some(items) = table.get("items").and_then(|v| v.as_table()) {
            for (key, value) in items {
                match parse_size(value) {
                    Ok(Some(n)) => {
                        rules.items.insert(normalize_item(key), n);
                    }
                    Ok(None) => {}
                    Err(e) => rules.warnings.push(format!("items.{key}: {e}, skipped")),
                }
            }
        }
        for key in table.keys() {
            if !matches!(key.as_str(), "settings" | "groups" | "items") {
                rules
                    .warnings
                    .push(format!("[{key}]: unknown section, ignored"));
            }
        }
        rules
    }

    /// The size the config asks for: the item's own entry, else its group's.
    pub fn wanted(&self, item: &str) -> Option<i64> {
        self.items.get(item).copied().or_else(|| {
            Group::ALL
                .iter()
                .find(|g| g.contains(item))
                .and_then(|g| self.groups.get(g).copied())
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decision {
    /// The size to set, None to leave the item's default alone.
    pub set: Option<i64>,
    /// The size the ledger must remember from now on (raised only).
    pub remember: Option<i64>,
    /// Why the config's wish was not followed, if it was not.
    pub note: Option<String>,
}

/// What to do with `item`, whose default components are `components`, given
/// the size this mod set for it before (`applied`, from the ledger).
pub fn decide(
    rules: &Rules,
    item: &str,
    components: &Map<String, Value>,
    applied: Option<i64>,
) -> Decision {
    let unchanged = Decision {
        set: None,
        remember: None,
        note: None,
    };
    // Items' reports carry a size for every item; without one we can't tell
    // raising from lowering, so leave it alone.
    let Some(vanilla) = components.get(STACK_SIZE).and_then(Value::as_i64) else {
        return unchanged;
    };
    let wanted = rules.wanted(item);
    if wanted.is_none() && applied.is_none() {
        return unchanged;
    }
    let mut note = None;
    let mut target = wanted.unwrap_or(vanilla);
    if target > vanilla
        && let Some(why) = blocked(item, components)
    {
        note = Some(format!("{target} refused, {why}"));
        target = vanilla;
    }
    if !rules.allow_lowering {
        let floor = vanilla.max(applied.unwrap_or(vanilla));
        if target < floor {
            note = Some(format!(
                "{target} would make it smaller than {floor} and existing stacks could lose \
                 items; kept {floor} (set allow_lowering = true to allow it)"
            ));
            target = floor;
        }
    }
    // Never stack an item with durability, whatever the ledger says.
    if components.contains_key(MAX_DAMAGE) {
        target = vanilla;
    }
    let target = target.clamp(1, MAX_SIZE.max(vanilla));
    Decision {
        set: (target != vanilla).then_some(target),
        remember: (target > vanilla && applied.is_none_or(|a| target > a)).then_some(target),
        note,
    }
}

/// Sizes this mod has applied, so a later config can't shrink them by accident.
#[derive(Debug, Default)]
pub struct Ledger {
    path: Option<PathBuf>,
    sizes: BTreeMap<String, i64>,
}

impl Ledger {
    pub fn load(path: Option<PathBuf>) -> Ledger {
        let sizes = path
            .as_deref()
            .and_then(|p| fs::read(p).ok())
            .and_then(|bytes| serde_json::from_slice::<BTreeMap<String, i64>>(&bytes).ok())
            .unwrap_or_default();
        Ledger { path, sizes }
    }

    pub fn get(&self, item: &str) -> Option<i64> {
        self.sizes.get(item).copied()
    }

    pub fn remember(&mut self, item: &str, size: i64) {
        self.sizes.insert(item.to_owned(), size);
        if let Some(path) = &self.path {
            let json = serde_json::to_vec_pretty(&self.sizes).unwrap_or_default();
            // Write beside, then rename: a crash mid-write never leaves half a file.
            let tmp = path.with_extension("json.tmp");
            let result = fs::write(&tmp, json).and_then(|()| fs::rename(&tmp, path));
            if let Err(e) = result {
                log::warn!(target: TARGET, "{LOG} could not save {}: {e}", path.display());
            }
        }
    }
}

/// What the game told us about an item when it read its defaults.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seen {
    /// The data pack's (vanilla's) size.
    pub vanilla: i64,
    /// The size in effect now, after this mod.
    pub now: i64,
    pub damageable: bool,
}

pub struct StackOptimized {
    rules: Rules,
    ledger: Mutex<Ledger>,
    logged: Mutex<HashSet<String>>,
    seen: Mutex<BTreeMap<String, Seen>>,
    /// Where the config lives; None in tests that keep it in memory.
    dir: Option<PathBuf>,
}

impl StackOptimized {
    pub fn new(rules: Rules, ledger: Ledger) -> StackOptimized {
        for warning in &rules.warnings {
            log::warn!(target: TARGET, "{LOG} {warning}");
        }
        if rules.print_table {
            log::info!(
                target: TARGET,
                "{LOG} groups: {:?}, items: {:?}, allow_lowering: {}",
                rules
                    .groups
                    .iter()
                    .map(|(g, n)| format!("{}={n}", g.name()))
                    .collect::<Vec<_>>(),
                rules.items,
                rules.allow_lowering
            );
        }
        StackOptimized {
            rules,
            ledger: Mutex::new(ledger),
            logged: Mutex::new(HashSet::new()),
            seen: Mutex::new(BTreeMap::new()),
            dir: None,
        }
    }

    /// Reads `config_dir/stackoptimized.toml`, writing the built-in default
    /// there when it is missing, and the ledger beside it.
    pub fn from_config_dir(dir: &Path) -> StackOptimized {
        let _ = fs::create_dir_all(dir);
        let config = dir.join(CONFIG_FILE);
        let text = match fs::read_to_string(&config) {
            Ok(text) => text,
            Err(_) => {
                if let Err(e) = fs::write(&config, DEFAULT_CONFIG) {
                    log::warn!(target: TARGET, "{LOG} could not write {}: {e}", config.display());
                }
                DEFAULT_CONFIG.to_owned()
            }
        };
        let rules = Rules::parse(&text);
        let mut m = StackOptimized::new(rules, Ledger::load(Some(dir.join(LEDGER_FILE))));
        m.dir = Some(dir.to_owned());
        m
    }

    pub fn apply(&self, item: &str, components: &mut Map<String, Value>) {
        let applied = self.ledger.lock().map(|l| l.get(item)).unwrap_or(None);
        let decision = decide(&self.rules, item, components, applied);
        if let Some(size) = decision.remember
            && let Ok(mut ledger) = self.ledger.lock()
        {
            ledger.remember(item, size);
        }
        if let Some(vanilla) = components.get(STACK_SIZE).and_then(Value::as_i64)
            && let Ok(mut seen) = self.seen.lock()
        {
            seen.insert(
                item.to_owned(),
                Seen {
                    vanilla,
                    now: decision.set.unwrap_or(vanilla),
                    damageable: components.contains_key(MAX_DAMAGE),
                },
            );
        }
        if let Some(size) = decision.set {
            components.insert(STACK_SIZE.into(), size.into());
        }
        // The game reads the defaults several times: say each thing once.
        if (decision.note.is_some() || (self.rules.print_table && decision.set.is_some()))
            && let Ok(mut logged) = self.logged.lock()
            && logged.insert(item.to_owned())
        {
            if let Some(note) = &decision.note {
                log::warn!(target: TARGET, "{LOG} {item}: {note}");
            }
            if let Some(size) = decision.set {
                log::info!(target: TARGET, "{LOG} {item}: max stack size {size}");
            }
        }
    }
}

/// The game's `config/` folder: the working directory's, as Horizon's own
/// `Game::native_config_dir` (beside `server.properties` on a server).
fn config_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_default().join("config")
}

static MOD: OnceLock<StackOptimized> = OnceLock::new();

/// The mod, read from the game's `config/` the first time it is asked.
pub fn get() -> &'static StackOptimized {
    MOD.get_or_init(|| StackOptimized::from_config_dir(&config_dir()))
}

/// Called by the hook for every item each time the game reads the defaults.
pub fn modify_default_components(item: &str, components: &mut Map<String, Value>) {
    get().apply(item, components);
}

pub mod edit;
pub mod gui;

#[cfg(test)]
mod tests;
