// New module tree
pub mod frontend;
pub mod ir;
pub mod backend;
pub mod stdlib;
pub mod driver;

// Diagnostics (absorbed from diagnostics.rs)
use std::path::Path;
use frontend::lexer::Span;

pub fn render_file_diagnostic(path: &Path, source: &str, message: &str, span: Span) -> String {
    let display_path = pretty_path(path);
    let line_text = source
        .lines()
        .nth(span.line.saturating_sub(1))
        .unwrap_or_default();
    let caret_column = span.column.max(1);
    let caret_padding = " ".repeat(caret_column.saturating_sub(1));
    format!(
        "{}\n --> {}:{}:{}\n  |\n{:>2} | {}\n  | {}^",
        message, display_path, span.line, span.column, span.line, line_text, caret_padding
    )
}

fn pretty_path(path: &Path) -> String {
    let raw = path.display().to_string();
    raw.strip_prefix("\\\\?\\").unwrap_or(&raw).to_string()
}

// Version (absorbed from version.rs)
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

// Legacy paths for existing tests and external users
pub use frontend::lexer;
pub use frontend::parser;
pub use frontend::semantic;
pub use frontend::semantic as warnings;
pub use backend::native as codegen;
pub use backend::llvm as llvm_ir;
pub use backend::llvm as llvm_backend;
pub use backend::llvm as avr_cbe_opt;
pub use stdlib as builtin_modules;
pub use stdlib as module_loader;
pub use driver::build;
pub use driver::build as build_module;
pub use driver::toolchain;
pub use ir as optimize;

// Legacy diagnostics and version module re-exports
pub mod diagnostics {
    pub use crate::render_file_diagnostic;
}

pub mod version {
    pub use crate::{RUNE_VERSION, RUNE_REPOSITORY, short_version, release_tag, display_version};
}
