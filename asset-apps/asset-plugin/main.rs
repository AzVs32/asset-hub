use std::path::Path;

mod tool;

use tool::{
    generate_manifest, seal_manifest, verify_manifest, verify_wasm_manifest, verify_web_manifest,
};

fn main() {
    if let Err(error) = run() {
        eprintln!("asset-plugin: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args().skip(1);
    let command = arguments.next().ok_or(
        "usage: asset-plugin gen manifest | asset-plugin <seal|verify|verify-wasm|verify-web> <manifest.json>",
    )?;
    if command == "gen" {
        if arguments.next().as_deref() != Some("manifest") || arguments.next().is_some() {
            return Err("usage: asset-plugin gen manifest".into());
        }
        let path = Path::new("manifest.json");
        let plugin = generate_manifest(path)?;
        println!(
            "generated plugin `{}` ({})",
            plugin.plugin_id(),
            path.display()
        );
        return Ok(());
    }
    let manifest = arguments.next().ok_or("missing manifest path")?;
    if arguments.next().is_some() {
        return Err("unexpected additional arguments".into());
    }
    let path = Path::new(&manifest);
    let plugin = match command.as_str() {
        "seal" => seal_manifest(path)?,
        "verify" => verify_manifest(path)?,
        "verify-wasm" => verify_wasm_manifest(path)?,
        "verify-web" => verify_web_manifest(path)?,
        _ => {
            return Err(format!(
                "unknown command `{command}`; expected `seal`, `verify`, `verify-wasm`, or `verify-web`"
            )
            .into());
        }
    };
    println!(
        "{} plugin `{}` ({})",
        if command == "seal" {
            "sealed"
        } else {
            "verified"
        },
        plugin.plugin_id(),
        path.display()
    );
    Ok(())
}
