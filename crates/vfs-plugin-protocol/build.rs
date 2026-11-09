fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=proto/plugin.proto");
    let mut config = prost_build::Config::new();
    config.protoc_executable(protoc_bin_vendored::protoc_bin_path()?);
    config.btree_map(["."]);
    config.boxed(".assethub.vfs.v4.Reply.result.description");
    config.file_descriptor_set_path(
        std::path::PathBuf::from(std::env::var("OUT_DIR")?).join("plugin.bin"),
    );
    config.compile_protos(&["proto/plugin.proto"], &["proto"])?;
    Ok(())
}
