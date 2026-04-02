pub const RUNE_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const RUNE_REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");

pub fn short_version() -> &'static str {
    RUNE_VERSION
}

pub fn release_tag() -> String {
    format!("v{}", short_version())
}

pub fn display_version() -> String {
    format!("Rune {}", short_version())
}
