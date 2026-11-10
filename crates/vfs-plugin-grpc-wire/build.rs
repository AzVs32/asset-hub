fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=../vfs-plugin-protocol/proto/plugin.proto");
    let mut config = tonic_prost_build::Config::new();
    config.protoc_executable(protoc_bin_vendored::protoc_bin_path()?);
    tonic_prost_build::configure()
        .extern_path(".assethub.vfs.v5", "::vfs_plugin_protocol::wire")
        .compile_with_config(
            config,
            &["../vfs-plugin-protocol/proto/plugin.proto"],
            &["../vfs-plugin-protocol/proto"],
        )?;
    if std::env::var_os("CARGO_FEATURE_STORAGE_CONTRACT").is_some() {
        println!("cargo:rerun-if-changed=proto/storage_contract.proto");
        let mut config = tonic_prost_build::Config::new();
        config.protoc_executable(protoc_bin_vendored::protoc_bin_path()?);
        tonic_prost_build::configure()
            .extern_path(".assethub.vfs.v5", "::vfs_plugin_protocol::wire")
            .compile_with_config(
                config,
                &["proto/storage_contract.proto"],
                &["proto", "../vfs-plugin-protocol/proto"],
            )?;
    }
    Ok(())
}
