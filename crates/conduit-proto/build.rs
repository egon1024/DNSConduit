fn main() -> Result<(), Box<dyn std::error::Error>> {
    std::env::set_var("PROTOC", protoc_bin_vendored::protoc_bin_path()?);
    let proto_root = "../../proto";
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(
            &[
                "conduit/v1/config.proto",
                "conduit/v1/control.proto",
            ],
            &[proto_root],
        )?;
    Ok(())
}
