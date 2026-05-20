pub mod config {
    include!(concat!(env!("OUT_DIR"), "/conduit.v1.rs"));
}
pub mod control {
    tonic::include_proto!("conduit.v1");
}
pub use config::*;
