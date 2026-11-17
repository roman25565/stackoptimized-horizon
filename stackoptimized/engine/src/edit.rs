//! Changes from `/setmaxstacksize` and `/stackgui`: written to the config
//! file at once, applied on the next start. Item defaults must not change
//! while the game runs (Horizon asks for the same answer every time it reads
//! them, and the players' clients read them too), so nothing changes live.

use super::*;

static WRITES: Mutex<()> = Mutex::new(());

/// What a config edit leads to, for one item, after the next restart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct After {
    pub size: i64,
    /// Why it isn't what the config says, in Ukrainian for the chat.
    pub why: Option<String>,
}

fn size_value(size: Option<i64>) -> String {
    size.map_or_else(|| "\"default\"".to_owned(), |n| n.to_string())
}

impl Rules {
    /// The config as a file. Comments of a hand-edited file are not kept;
    /// the header says what the keys mean.
    pub fn to_toml(&self) -> String {
        let mut out = String::from(
            "# StackOptimized for Horizon. /stackgui and /setmaxstacksize edit this file;\n\
             # you can edit it by hand too. New values apply after a server restart.\n\
             # Size: a number 1..99 or \"default\". Filled buckets, bundles, enchanted books\n\
             # and items with durability are never raised (dupes or lost items).\n\
             # allow_lowering = true allows smaller stacks (back up the world first).\n\n\
             [settings]\n",
        );
        out += &format!("print_table = {}\n", self.print_table);
        out += &format!("allow_lowering = {}\n\n[groups]\n", self.allow_lowering);
        for g in Group::ALL {
            out += &format!(
                "{} = {}\n",
                g.name(),
                size_value(self.groups.get(&g).copied())
            );
        }
        out += "\n[items]\n";
        for (item, n) in &self.items {
            out += &format!("{} = {n}\n", toml::Value::String(item.clone()));
        }
        out
    }
}

impl StackOptimized {
    /// The rules in effect since the start.
    pub fn active_rules(&self) -> &Rules {
        &self.rules
    }

    pub fn seen(&self, item: &str) -> Option<Seen> {
        self.seen.lock().ok()?.get(item).copied()
    }

    pub fn applied(&self, item: &str) -> Option<i64> {
        self.ledger.lock().ok()?.get(item)
    }

    fn config_path(&self) -> Result<PathBuf, String> {
        self.dir
            .as_ref()
            .map(|d| d.join(CONFIG_FILE))
            .ok_or_else(|| "no config folder".to_owned())
    }

    /// The rules as the file says now: what the next start will use.
    pub fn pending(&self) -> Result<Rules, String> {
        let path = self.config_path()?;
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => DEFAULT_CONFIG.to_owned(),
            Err(e) => return Err(format!("could not read {}: {e}", path.display())),
        };
        Ok(Rules::parse(&text))
    }

    /// Reads the file, lets `change` edit the rules and writes them back.
    /// A file with mistakes in it is never rewritten: what it holds would be
    /// lost, so the admin fixes it by hand first.
    pub fn edit<T>(
        &self,
        change: impl FnOnce(&mut Rules) -> Result<T, String>,
    ) -> Result<T, String> {
        let _one = WRITES.lock().unwrap_or_else(|e| e.into_inner());
        let mut rules = self.pending()?;
        if let Some(w) = rules.warnings.first() {
            return Err(format!(
                "config/{CONFIG_FILE} has an error ({w}); fix the file by hand, then try again"
            ));
        }
        let out = change(&mut rules)?;
        let path = self.config_path()?;
        let tmp = path.with_extension("toml.tmp");
        fs::write(&tmp, rules.to_toml())
            .and_then(|()| fs::rename(&tmp, &path))
            .map_err(|e| format!("could not write {}: {e}", path.display()))?;
        Ok(out)
    }

    /// The item's size after a restart with `rules`, as far as the game has
    /// told us about it (None for an item it hasn't read).
    pub fn after_restart(&self, rules: &Rules, item: &str) -> Option<After> {
        let seen = self.seen(item)?;
        let mut components = Map::new();
        components.insert(STACK_SIZE.into(), seen.vanilla.into());
        if seen.damageable {
            components.insert(MAX_DAMAGE.into(), 1.into());
        }
        let applied = self.applied(item);
        let d = decide(rules, item, &components, applied);
        let size = d.set.unwrap_or(seen.vanilla);
        let wanted = rules.wanted(item).unwrap_or(seen.vanilla);
        let why = (size != wanted).then(|| {
            if wanted > size {
                match blocked(item, &components) {
                    Some(_) => "this item can't stack higher: dupes or lost items".to_owned(),
                    None => format!("no more than {MAX_SIZE}"),
                }
            } else {
                format!(
                    "stays at least {size} while allow_lowering = false (items in stacks \
                     bigger than the new size would be lost)"
                )
            }
        });
        Some(After { size, why })
    }

    /// `/setmaxstacksize`: the item's own entry, written for the next start.
    /// `None` removes the entry. The answer is for the player's chat.
    pub fn set_item(&self, item: &str, size: Option<i64>) -> Result<String, String> {
        if let Some(n) = size
            && !(1..=MAX_SIZE).contains(&n)
        {
            return Err(format!("the size must be from 1 to {MAX_SIZE}"));
        }
        if let (Some(n), Some(seen)) = (size, self.seen(item)) {
            let mut components = Map::new();
            if seen.damageable {
                components.insert(MAX_DAMAGE.into(), 1.into());
            }
            if n > seen.vanilla
                && let Some(why) = blocked(item, &components)
            {
                return Err(format!(
                    "{item} can't stack higher than {}: {why}",
                    seen.vanilla
                ));
            }
        }
        let rules = self.edit(|rules| {
            match size {
                Some(n) => rules.items.insert(item.to_owned(), n),
                None => rules.items.remove(item),
            };
            Ok(rules.clone())
        })?;
        Ok(self.describe(&rules, item))
    }

    /// `minecraft:x: now A, after a server restart B` and why, for the chat.
    pub fn describe(&self, rules: &Rules, item: &str) -> String {
        let now = self.seen(item).map(|s| s.now);
        match (now, self.after_restart(rules, item)) {
            (Some(now), Some(after)) => {
                let mut line = format!(
                    "{item}: now {now}, after a server restart {}",
                    after.size
                );
                if let Some(why) = after.why {
                    line += &format!(" ({why})");
                }
                line
            }
            _ => format!("{item}: saved, applies after a server restart"),
        }
    }
}
