// HEAD of horizon_world::item_defaults::native_modify_default_components.
// Returning unit lets the original body (and other mods) run afterwards.
fn stackoptimized_defaults(item: &str, components: &mut Map<String, Value>) {
    stackoptimized_engine::modify_default_components(item, components);
}
