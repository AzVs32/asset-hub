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
