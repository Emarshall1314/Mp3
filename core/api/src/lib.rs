//! `cadenza-api`: the only crate app shells depend on.
//!
//! Types here stay plain (no lifetimes, no generics in public signatures) so the
//! same API can later be exported through UniFFI if a native mobile UI is needed.

use serde::Serialize;

/// Facts about the running build, shown in About and attached to bug reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildInfo {
    pub name: String,
    pub version: String,
    pub target_os: String,
    pub target_arch: String,
    pub debug: bool,
}

pub fn build_info() -> BuildInfo {
    BuildInfo {
        name: "Cadenza".to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        target_os: std::env::consts::OS.to_owned(),
        target_arch: std::env::consts::ARCH.to_owned(),
        debug: cfg!(debug_assertions),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_info_reports_this_crate_version() {
        let info = build_info();
        assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
        assert_eq!(info.target_os, std::env::consts::OS);
    }
}
