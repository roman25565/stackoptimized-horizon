// A player's command on the server: StackOptimized's, when it is one.
fn stackoptimized_command(
    game: &mut crate::game::Game<'_>,
    player: &crate::game::native_server::NativeServerPlayer,
    command: &str,
    ran: &mut bool,
) {
    if !*ran {
        crate::game::stackoptimized_glue::perform(game, player, command, ran);
    }
}
