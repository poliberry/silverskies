//! The app version shown to people: the workspace version plus, for
//! release builds, the build number the Release workflow assigns (it sets
//! `SS_BUILD` while compiling; builds start at 1 for each version).

/// The workspace version, e.g. `2.0.2`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The release build number, when this binary came from the Release workflow.
pub const BUILD: Option<&str> = option_env!("SS_BUILD");

/// `2.0.2 (build 1)`, or `2.0.2 (development build)` for local builds.
pub fn version_label() -> String {
    label_for(VERSION, BUILD)
}

fn label_for(version: &str, build: Option<&str>) -> String {
    match build.map(str::trim).filter(|b| !b.is_empty()) {
        Some(b) => format!("{version} (build {b})"),
        None => format!("{version} (development build)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_release_and_local_builds() {
        assert_eq!(label_for("2.0.2", Some("1")), "2.0.2 (build 1)");
        assert_eq!(label_for("2.0.2", None), "2.0.2 (development build)");
        assert_eq!(label_for("2.0.2", Some(" ")), "2.0.2 (development build)");
    }
}
