// Each server tick: open the menus asked for by /stackgui.
fn stackoptimized_tick(game: &mut crate::game::Game<'_>) {
    crate::game::stackoptimized_glue::tick(game);
}
