use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod tool;

use tool::{
    generate_manifest, generate_schema, seal_manifest, verify_manifest, verify_wasm_manifest,
    verify_web_manifest,
};

#[derive(Debug, Parser)]
#[command(
    name = "asset-plugin",
    version,
    about = "Build and verify Asset Hub plugin packages"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Generate an editable plugin development file.
    Gen {
        #[command(subcommand)]
        command: GenerateCommand,
    },
    /// Generate Wasm and Web integrity data and update the manifest.
    Seal {
        /// Path to the plugin manifest JSON file.
        manifest: PathBuf,
    },
    /// Verify the complete sealed plugin package.
    Verify {
        /// Path to the plugin manifest JSON file.
        manifest: PathBuf,
    },
    /// Verify only the Wasm artifact.
    VerifyWasm {
        /// Path to the plugin manifest JSON file.
        manifest: PathBuf,
    },
    /// Verify only the Web artifact set.
    VerifyWeb {
        /// Path to the plugin manifest JSON file.
        manifest: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
enum GenerateCommand {
    /// Copy the canonical Manifest V3 draft to ./manifest.json.
    Manifest,
    /// Copy the Manifest V3 JSON Schema to ./manifest.schema.json.
    Schema,
}

fn main() {
    if let Err(error) = run(Cli::parse()) {
        eprintln!("asset-plugin: {error}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    if matches!(
        &cli.command,
        Command::Gen {
            command: GenerateCommand::Schema
        }
    ) {
        let path = PathBuf::from("manifest.schema.json");
        generate_schema(&path)?;
        println!("generated plugin manifest schema ({})", path.display());
        return Ok(());
    }
    let (operation, path, plugin) = match cli.command {
        Command::Gen {
            command: GenerateCommand::Manifest,
        } => {
            let path = PathBuf::from("manifest.json");
            let plugin = generate_manifest(&path)?;
            ("generated", path, plugin)
        }
        Command::Gen {
            command: GenerateCommand::Schema,
        } => unreachable!("schema generation returned above"),
        Command::Seal { manifest } => {
            let plugin = seal_manifest(&manifest)?;
            ("sealed", manifest, plugin)
        }
        Command::Verify { manifest } => {
            let plugin = verify_manifest(&manifest)?;
            ("verified", manifest, plugin)
        }
        Command::VerifyWasm { manifest } => {
            let plugin = verify_wasm_manifest(&manifest)?;
            ("verified", manifest, plugin)
        }
        Command::VerifyWeb { manifest } => {
            let plugin = verify_web_manifest(&manifest)?;
            ("verified", manifest, plugin)
        }
    };
    println!(
        "{} plugin `{}` ({})",
        operation,
        plugin.plugin_id(),
        path.display()
    );
    Ok(())
}

#[cfg(test)]
mod cli_tests {
    use super::*;

    #[test]
    fn parses_nested_generate_and_typed_manifest_paths() {
        let generated = Cli::try_parse_from(["asset-plugin", "gen", "manifest"]).unwrap();
        assert!(matches!(
            generated.command,
            Command::Gen {
                command: GenerateCommand::Manifest
            }
        ));

        let sealed = Cli::try_parse_from(["asset-plugin", "seal", "plugin.json"]).unwrap();
        assert!(matches!(
            sealed.command,
            Command::Seal { manifest } if manifest == std::path::Path::new("plugin.json")
        ));

        let schema = Cli::try_parse_from(["asset-plugin", "gen", "schema"]).unwrap();
        assert!(matches!(
            schema.command,
            Command::Gen {
                command: GenerateCommand::Schema
            }
        ));
    }
}
