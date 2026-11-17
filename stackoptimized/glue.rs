// StackOptimized's commands and menu in the server's game, added as
// `game::stackoptimized_glue`. The rules live in the mod's crate
// (`horizon_world::stackoptimized::engine`); this only talks to the game.
// Nothing here changes a stack size while the game runs: commands and the
// menu write the config file, which the next start reads.

use crate::game::{
    Game,
    native_menus::native_menu_inventory_slot,
    native_server::{NativeCommand, NativeServerPlayer},
};
use horizon_client::{
    items::ItemStack,
    lan::data::component_nbt,
    text::{self, Text},
};
use horizon_world::stackoptimized::engine::{
    self,
    gui::{self, Cell, Target},
};
use std::{
    collections::HashMap,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering::Relaxed},
    },
};

/// The operator level the commands need (the old plugin checked "op").
const LEVEL: u8 = 2;

struct View {
    window: Option<i32>,
    targets: Vec<Target>,
}

#[derive(Default)]
struct State {
    views: HashMap<u128, View>,
    /// Asked in a command, opened at the next tick (menus can't open while
    /// the player's state is swapped in).
    pending: Vec<u128>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static ACTIVE: AtomicBool = AtomicBool::new(false);

fn with_state<T>(f: impl FnOnce(&mut State) -> T) -> T {
    let mut s = STATE.lock().unwrap_or_else(|e| e.into_inner());
    let out = f(s.get_or_insert_with(State::default));
    let s = s.as_ref().expect("made");
    ACTIVE.store(!s.views.is_empty() || !s.pending.is_empty(), Relaxed);
    out
}

pub fn commands(commands: &mut Vec<NativeCommand>) {
    // Horizon allows only lowercase command words, so no `setMaxStackSize`.
    for usage in [
        "setmaxstacksize <size:integer>",
        "stackgui",
        "name",
    ] {
        commands.push(NativeCommand::op(usage, LEVEL));
    }
}

fn ok(game: &mut Game<'_>, line: String) {
    game.native_feedback(Text::literal(line).with_color(text::GREEN));
}

fn error(game: &mut Game<'_>, line: String) {
    game.native_feedback(Text::literal(line).with_color(text::RED));
}

pub(in crate::game) fn perform(
    game: &mut Game<'_>,
    player: &NativeServerPlayer,
    line: &str,
    ran: &mut bool,
) {
    let mut words = line.split_whitespace();
    let word = words.next().unwrap_or_default();
    if !matches!(
        word,
        "setMaxStackSize" | "setmaxstacksize" | "stackgui" | "name"
    ) {
        return;
    }
    *ran = true;
    if player.permission < LEVEL {
        error(game, "You don't have permission to use this command.".into());
        return;
    }
    let m = engine::get();
    match word {
        "stackgui" => {
            with_state(|s| {
                s.pending.retain(|p| *p != player.uuid);
                s.pending.push(player.uuid);
            });
        }
        "name" => match game.native_held() {
            Some((item, _, _)) => {
                let line = match m.pending() {
                    Ok(rules) => m.describe(&rules, &item),
                    Err(e) => e,
                };
                ok(game, line);
            }
            None => error(game, "Hold an item in your hand.".into()),
        },
        _ => {
            let Some((item, _, _)) = game.native_held() else {
                error(game, "Hold an item in your hand.".into());
                return;
            };
            let size = match words.next().map(str::parse::<i64>) {
                Some(Ok(n)) => n,
                _ => {
                    error(game, "Usage: /setmaxstacksize <1..99>".into());
                    return;
                }
            };
            match m.set_item(&item, Some(size)) {
                Ok(line) => ok(game, line),
                Err(e) => error(game, e),
            }
        }
    }
}

fn stack(cell: &Cell) -> ItemStack {
    let mut s = ItemStack {
        item: cell.icon.as_str().into(),
        count: cell.count,
        damage: 0,
        extra: None,
    };
    let title = serde_json::json!({ "text": cell.title, "italic": false, "color": "gold" });
    s.data_mut().custom_name = Some(component_nbt(&title));
    let lore = cell
        .lore
        .iter()
        .map(|(t, c)| component_nbt(&serde_json::json!({ "text": t, "italic": false, "color": c })))
        .collect();
    s.data_mut()
        .set_other("minecraft:lore", Some(horizon_world::nbt::Tag::List(lore)));
    s
}

/// The menu's items and what each slot stands for, from the file.
fn layout() -> Result<(Vec<Option<ItemStack>>, Vec<Target>), String> {
    let m = engine::get();
    let cells = m.cells(&m.pending()?);
    let items = cells.iter().map(|c| Some(stack(c))).collect();
    Ok((items, cells.into_iter().map(|c| c.target).collect()))
}

/// Each tick: one atomic load while no menu is asked for.
pub(in crate::game) fn tick(game: &mut Game<'_>) {
    if !ACTIVE.load(Relaxed) {
        return;
    }
    let pending = with_state(|s| std::mem::take(&mut s.pending));
    for uuid in pending {
        match layout() {
            Ok((items, targets)) => {
                let title = Text::literal(gui::TITLE);
                let window = game.native_open_menu(uuid, gui::ROWS, title, items);
                if window.is_some() {
                    with_state(|s| {
                        s.views.insert(uuid, View { window, targets });
                    });
                }
            }
            Err(e) => {
                game.native_tell(uuid, Text::literal(e).with_color(text::RED));
            }
        }
    }
}

/// A click in the menu: true when it was ours.
pub(in crate::game) fn menu_clicked(
    game: &mut Game<'_>,
    uuid: u128,
    window: i32,
    slot: i32,
    button: i32,
    mode: i32,
) -> bool {
    let target = with_state(|s| {
        let view = s.views.get(&uuid).filter(|v| v.window == Some(window))?;
        Some(usize::try_from(slot).ok().and_then(|i| view.targets.get(i).cloned()))
    });
    let Some(target) = target else {
        return false;
    };
    let m = engine::get();
    let result = match target {
        Some(target) => m.click_cell(&target, gui::click(mode, button)),
        // The player's own items below the menu: add the clicked one.
        None => match native_menu_inventory_slot(gui::SIZE, slot)
            .and_then(|i| game.native_player_stacks(uuid)?.get(i).cloned().flatten())
        {
            Some(s) if matches!(mode, 0 | 1) => m.add_item(&s.item),
            _ => Ok(None),
        },
    };
    match result {
        Ok(Some(line)) => {
            game.native_tell(uuid, Text::literal(line).with_color(text::GREEN));
        }
        Ok(None) => {}
        Err(e) => {
            game.native_tell(uuid, Text::literal(e).with_color(text::RED));
        }
    }
    if let Ok((items, targets)) = layout() {
        with_state(|s| {
            if let Some(v) = s.views.get_mut(&uuid) {
                v.targets = targets;
            }
        });
        game.native_set_menu_items(uuid, items);
    }
    true
}

/// The menu closed: true when it was ours.
pub(in crate::game) fn menu_closed(uuid: u128, window: i32) -> bool {
    with_state(|s| {
        let ours = s.views.get(&uuid).is_some_and(|v| v.window == Some(window));
        if ours {
            s.views.remove(&uuid);
        }
        ours
    })
}
