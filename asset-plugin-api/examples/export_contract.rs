use asset_plugin_api::spec;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "spec".to_string());
    let output = Path::new(&output);
    std::fs::create_dir_all(output)?;
    write_json(output.join("contract-v3.json"), &spec::contract_catalog())?;
    for (name, schema) in spec::schemas() {
        write_json(output.join(name), &schema)?;
    }
    if let Some(typescript) = std::env::args().nth(2) {
        std::fs::write(typescript, spec::typescript_catalog_module())?;
    }
    Ok(())
}

fn write_json(
    path: impl AsRef<Path>,
    value: &impl serde::Serialize,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    std::fs::write(path, bytes)?;
    Ok(())
}
