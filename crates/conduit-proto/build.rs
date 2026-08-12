fn main() -> Result<(), Box<dyn std::error::Error>> {
    std::env::set_var("PROTOC", protoc_bin_vendored::protoc_bin_path()?);
    let proto_root = "../../proto";
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR")?);
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .file_descriptor_set_path(out_dir.join("conduit_descriptor.bin"))
        .compile_protos(
            &[
                "conduit/v1/config.proto",
                "conduit/v1/control.proto",
                "conduit/v1/health.proto",
                "conduit/v1/pools.proto",
            ],
            &[proto_root],
        )?;
    Ok(())
}
