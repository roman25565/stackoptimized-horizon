// StackOptimized's commands in every player's command tree.
fn stackoptimized_register(commands: &mut Vec<crate::game::native_server::NativeCommand>) {
    crate::game::stackoptimized_glue::commands(commands);
}
