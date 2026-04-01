use std::path::Path;

use crate::lexer::Span;

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
