# StackOptimized for Horizon (Minecraft 26.3)

A native Horizon Rust mod (`.hnmod`), a port of the Spigot plugin StackOptimized 1.2.
Sets the maximum stack size for any item or group of items.

## How it works

The mod hooks into Horizon's `modify_default_components` (API 79, the NeoForge equivalent of
`ModifyDefaultComponentsEvent`) and changes the item's **default** `minecraft:max_stack_size`
component. So the game engine treats the new size as "vanilla" for that item.

Differences from the old plugin:

| Spigot plugin | Horizon mod |
|---|---|
| Set the component on each stack separately, only when a player picked something up | Applies to all items everywhere: chests, hoppers, crafting, loot, the ground |
| The component stayed in the items forever (hence `/resetstacks`) | Nothing is stored in the items; remove the mod and everything is vanilla |
| Limit 1..99 | Same limit 1..99 |

## Protection against dupes and item loss

I went through the Horizon engine code and found places where stacking certain items leads to
loss or duplication. The mod **never increases** the stack size of such items (even if the config says so)
and prints a warning to the console:

- **Items with durability** (tools, armour, weapons): the engine rejects such stacks entirely.
- **Full buckets** (water, lava, milk, powder snow, buckets of fish/axolotl): when emptied,
  the whole stack in hand is replaced by one empty bucket (`game.rs`, `creatures.rs`). The empty bucket
  (`minecraft:bucket`) can be increased.
- **Bundles**: items put into a stack of bundles end up in every bundle of the stack
  (a dupe, `menu/bundle.rs` doesn't check the count).
- **Enchanted books**: the anvil takes the whole stack of books from the slot (`menu.rs`, `anvil_take`).

Second rule: **the mod doesn't lower a stack size**, neither below vanilla nor below a size it once
set. Reason: containers trim a stack to the item's limit (`menu.rs`, `put`), and the extra
items would vanish. The mod remembers the sizes it has set in
`config/stackoptimized-applied.json`. To really lower a size, set `allow_lowering = true`
(and back up the world).

## Config

`config/stackoptimized.toml` in the server folder (next to `server.properties`). If the file is missing,
the mod creates it on first start from the built-in values
(`stackoptimized/engine/data/stackoptimized.toml`). Changes apply after a restart.

```toml
[settings]
print_table = true
allow_lowering = false

[groups]          # a number 1..99 or "default"
shulker_boxes = 64
potions = 16
beds = 16

[items]           # "minecraft:snowball", "snowball" or SNOWBALL
"minecraft:ender_pearl" = 64
```

The old names from the plugin's `config.yml` are understood too (`Skulkers`, `Music_Disc`, `SNOWBALL`...).

**Player clients**: the Horizon client should have this mod with the same values too. Without it
(a vanilla client or Horizon without the mod) the player sees vanilla limits. Items don't
dupe because of that, since the server decides everything, but the inventory "jumps". Simplest: put your values into
the built-in `engine/data/stackoptimized.toml` before building and give players the same mod.

## Building and running (Windows)

You need the Horizon launcher. Building a native mod runs your own code on your PC, so you have to pass
the `--trust-native-code` flag yourself.

**Test server**:

```powershell
horizon-launcher versions --installed     # find the TAG of the installed version
horizon-launcher native-build .\stackoptimized --server-output .\test-engine --version TAG --trust-native-code
mkdir test-server; cd test-server
horizon-server init --eula --world test --port 25570 --offline --yes
horizon-server config set engine.path=<full path>\test-engine\horizon-engine.exe
horizon-server run
```

The first build takes 5–30 minutes (the launcher downloads Rust itself), later ones about a minute.

**Client**: Mods → Open folder in the instance you want, put the `stackoptimized` folder there, then Build;
or `horizon-launcher native-build .\stackoptimized --instance NAME --trust-native-code`.

## What to check in game

1. The server console has lines `[StackOptimized] minecraft:...: max stack size N`.
2. An increased item (e.g. potions 16): stacks to 16 in hand, chests, shulkers; a hopper moves
   them one by one; Shift-click, hotbar number swap, dropping with Q / Ctrl+Q, picking up from the ground.
3. Drinking one potion from a stack of 16 leaves 15 potions + 1 bottle.
4. Setting `water_bucket = 16`: a warning in the console, buckets stay at 1.
5. Setting `potions = 8` after 16: a warning in the console, it stays at 16.
6. Restarting the server: the sizes are the same, item counts in chests haven't changed.

## Commands (operators level 2+)

The commands work as in the plugin, but **change nothing immediately**: they write values into
`config/stackoptimized.toml`, and they take effect after a server restart. Horizon requires that stack
sizes don't change while the game runs (and that client and server have the same values).

- `/setmaxstacksize <1..99>` (Horizon only accepts lowercase commands): size for the item in hand. The reply says
  what it is now and what it will be after the restart. Dangerous items (full buckets, bundles, books,
  items with durability) are refused.
- `/stackgui`: a chest menu with groups and individual items. LMB +1, RMB -1, Shift+LMB +10,
  Shift+RMB -10, middle click or Q removes the entry; clicking your own item below adds it. Each
  item shows "Now" and "After restart". A click in the menu never moves real items.
- `/name`: the id of the item in hand and its size now / after restart.
- If the config has an error (broken TOML), the commands don't overwrite it but ask you to fix it
  by hand: otherwise your entries would be lost.
- `/resetstacks` is no longer needed: the mod writes nothing into items.

Items in a world carried over from Spigot may still carry the old plugin's component; it
takes priority over the mod, so such stacks keep the size they had.

## Files

```
stackoptimized/
  horizon-mixin.toml        manifest: HEAD hook modify_default_components + crate
  defaults.rs               the size hook: passes the item to the crate
  glue.rs                   in-game commands and menu (module game::stackoptimized_glue)
  register.rs, command.rs, tick.rs, menu_clicked.rs, menu_closed.rs   API 4/8/10 hooks
  reexport.rs               exposes the crate to the game as horizon_world::stackoptimized
  engine/
    Cargo.toml              dependencies only from Horizon's Cargo.lock (serde_json, toml, log)
    data/stackoptimized.toml  default config
    src/lib.rs              config, groups, safety rules, size journal
    src/edit.rs, src/gui.rs   writing the config from commands, menu model
    src/tests.rs            18 tests (cargo test in the engine folder)
```

## License

MIT, see [LICENSE](LICENSE). A mod for [Horizon](https://github.com/horizon-rust/horizon) (MIT, Ludovic Marie and the Horizon contributors).

Unofficial project, not affiliated with Mojang Studios or Microsoft. Minecraft is a trademark of Mojang Synergies AB.
