//! `/stackgui`: the old plugin's chest menu, as data the game glue turns
//! into items. Each click edits the config file for the next start; the menu
//! never moves a real item (Horizon's mod menus move nothing on a click).

use super::*;

pub const ROWS: u8 = 6;
pub const SIZE: usize = 54;
pub const TITLE: &str = "StackOptimized (after restart)";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Group(Group),
    Item(String),
}

/// One slot of the menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cell {
    pub target: Target,
    pub icon: String,
    pub title: String,
    /// Lines under the name, each with a colour name (`yellow`, `gray`...).
    pub lore: Vec<(String, &'static str)>,
    pub count: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Click {
    Change(i64),
    Remove,
    Nothing,
}

/// The click's `ClickType` id and button, as the old plugin read them:
/// left +1, right -1, shift-left +10, shift-right -10, middle or drop removes.
pub fn click(mode: i32, button: i32) -> Click {
    match (mode, button) {
        (0, 0) => Click::Change(1),
        (0, 1) => Click::Change(-1),
        (1, 0) => Click::Change(10),
        (1, 1) => Click::Change(-10),
        (3, _) | (4, _) => Click::Remove,
        _ => Click::Nothing,
    }
}

impl Group {
    /// The item the menu shows for the group.
    pub fn icon(self) -> &'static str {
        match self {
            Group::ShulkerBoxes => "minecraft:shulker_box",
            Group::Beds => "minecraft:red_bed",
            Group::Potions => "minecraft:potion",
            Group::EnchantedBooks => "minecraft:enchanted_book",
            Group::Minecarts => "minecraft:minecart",
            Group::Boats => "minecraft:oak_boat",
            Group::Banners => "minecraft:white_banner",
            Group::MusicDiscs => "minecraft:music_disc_cat",
            Group::Buckets => "minecraft:bucket",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Group::ShulkerBoxes => "Shulker boxes",
            Group::Beds => "Beds",
            Group::Potions => "Potions",
            Group::EnchantedBooks => "Enchanted books",
            Group::Minecarts => "Minecarts",
            Group::Boats => "Boats",
            Group::Banners => "Banners",
            Group::MusicDiscs => "Music discs",
            Group::Buckets => "Empty bucket",
        }
    }
}

fn size_text(size: Option<i64>) -> String {
    size.map_or_else(|| "vanilla".to_owned(), |n| n.to_string())
}

const HELP: [&str; 5] = [
    "Left click: +1, right click: -1",
    "Shift+left: +10, Shift+right: -10",
    "Middle click or Q: remove (vanilla)",
    "Click your own item below: add it",
    "Changes apply after a server restart",
];

impl StackOptimized {
    /// The menu's slots for the rules in the file (`pending`): the groups,
    /// then the items with their own entry, as many as fit.
    pub fn cells(&self, pending: &Rules) -> Vec<Cell> {
        let mut cells = Vec::new();
        for g in Group::ALL {
            let now = self.rules.groups.get(&g).copied();
            let after = pending.groups.get(&g).copied();
            let mut lore = vec![
                (format!("Now: {}", size_text(now)), "gray"),
                (format!("After restart: {}", size_text(after)), "yellow"),
            ];
            if g == Group::EnchantedBooks {
                lore.push(("Blocked: the anvil takes the whole stack".into(), "red"));
            }
            if g == Group::Buckets {
                lore.push(("Filled buckets are blocked (lost items)".into(), "red"));
            }
            lore.extend(HELP.map(|h| (h.to_owned(), "dark_gray")));
            cells.push(Cell {
                target: Target::Group(g),
                icon: g.icon().into(),
                title: g.title().into(),
                lore,
                count: after.unwrap_or(1).clamp(1, MAX_SIZE) as u32,
            });
        }
        for (item, n) in &pending.items {
            if cells.len() >= SIZE {
                break;
            }
            let now = self.seen(item).map(|s| s.now);
            let mut lore = vec![
                (format!("Now: {}", size_text(now)), "gray"),
                (format!("After restart: {n}"), "yellow"),
            ];
            if let Some(why) = self.after_restart(pending, item).and_then(|a| a.why) {
                lore.push((format!("Warning: {why}"), "red"));
            }
            lore.extend(HELP.map(|h| (h.to_owned(), "dark_gray")));
            cells.push(Cell {
                target: Target::Item(item.clone()),
                icon: item.clone(),
                title: item.clone(),
                lore,
                count: (*n).clamp(1, MAX_SIZE) as u32,
            });
        }
        cells
    }

    /// A click on a cell: the file changes, the answer is for the chat.
    pub fn click_cell(&self, target: &Target, click: Click) -> Result<Option<String>, String> {
        let delta = match click {
            Click::Nothing => return Ok(None),
            Click::Remove => None,
            Click::Change(d) => Some(d),
        };
        match target {
            Target::Item(item) => {
                let current = self
                    .pending()?
                    .items
                    .get(item)
                    .copied()
                    .or_else(|| self.seen(item).map(|s| s.vanilla))
                    .unwrap_or(1);
                let size = delta.map(|d| (current + d).clamp(1, MAX_SIZE));
                self.set_item(item, size).map(Some)
            }
            Target::Group(g) => {
                if *g == Group::EnchantedBooks && delta.is_some() {
                    return Err("enchanted books don't stack: the anvil takes the whole stack".into());
                }
                let base = self.seen(g.icon()).map_or(1, |s| s.vanilla);
                let after = self.edit(|rules| match delta {
                    Some(d) => {
                        let current = rules.groups.get(g).copied().unwrap_or(base);
                        let n = (current + d).clamp(1, MAX_SIZE);
                        rules.groups.insert(*g, n);
                        Ok(Some(n))
                    }
                    None => {
                        rules.groups.remove(g);
                        Ok(None)
                    }
                })?;
                Ok(Some(format!(
                    "{}: after a server restart {}",
                    g.title(),
                    size_text(after)
                )))
            }
        }
    }

    /// A click on one of the player's own items below the menu: adds it
    /// with the size it has now, to change from there.
    pub fn add_item(&self, item: &str) -> Result<Option<String>, String> {
        if self.pending()?.items.contains_key(item) {
            return Ok(None);
        }
        let now = self.seen(item).map_or(1, |s| s.now);
        self.set_item(item, Some(now)).map(Some)
    }
}
