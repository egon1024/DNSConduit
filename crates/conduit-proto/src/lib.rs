pub mod config {
    include!(concat!(env!("OUT_DIR"), "/conduit.v1.rs"));
}
pub mod control {
    tonic::include_proto!("conduit.v1");
}
pub const FILE_DESCRIPTOR_SET: &[u8] = tonic::include_file_descriptor_set!("conduit_descriptor");
pub use config::*;
