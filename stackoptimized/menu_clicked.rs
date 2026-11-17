// A click in a menu a native mod opened: ours, when it is /stackgui's.
fn stackoptimized_menu_clicked(
    game: &mut crate::game::Game<'_>,
    uuid: u128,
    window: i32,
    slot: i32,
    button: i32,
    mode: i32,
) {
    crate::game::stackoptimized_glue::menu_clicked(game, uuid, window, slot, button, mode);
}
