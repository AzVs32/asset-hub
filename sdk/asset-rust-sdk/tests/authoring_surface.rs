use asset_rust_sdk::{Diagnostic, ResourceContext, ResourceResponse, Result};

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
}
