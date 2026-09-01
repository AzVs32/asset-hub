use asset_rust_sdk::{
    Diagnostic, ResourceContentState, ResourceContext, ResourceEffectiveState,
    ResourceLifecycleState, ResourceResponse, ResourceSnapshot, Result,
};

fn runtime_neutral_handler(_context: ResourceContext) -> Result<ResourceResponse> {
    Ok(
        ResourceResponse::without_view().diagnostic(Diagnostic::info(
            "example.ready",
            "runtime-neutral authoring surface is available",
        )),
    )
}

#[test]
fn authoring_surface_is_available_without_a_runtime_adapter() {
    let _handler: fn(ResourceContext) -> Result<ResourceResponse> = runtime_neutral_handler;
    let _state_reader: fn(ResourceSnapshot<'_>) = read_resource_state;
}

fn read_resource_state(resource: ResourceSnapshot<'_>) {
    let state = resource.state();
    let _: ResourceLifecycleState<'_> = state.lifecycle();
    let _: ResourceContentState = state.content();
    let _: ResourceEffectiveState = state.effective();
}
