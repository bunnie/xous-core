// Versioning information is kept in a separate file, attached to a small, well-known server in the Xous System
// This is a trade-off between rebuild times and flexibility.
// This was autogenerated by xtask/src/main.rs:print_header(). Do not edit manually.

pub(crate) fn get_version() -> crate::api::VersionString {
    let mut v = crate::api::VersionString {
        version: xous_ipc::String::new()
    };
    v.version.append(SEMVER).ok();
    #[cfg(not(feature="no-timestamp"))]
    v.version.append("\n").ok();
    #[cfg(not(feature="no-timestamp"))]
    v.version.append(TIMESTAMP).ok();
    v
}
#[allow(dead_code)]
pub const TIMESTAMP: &'static str = "Wed, 05 Oct 2022 18:25:35 +0800";
pub const SEMVER: &'static str = "v0.9.9-441-g113d4c2d";
