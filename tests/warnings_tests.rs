use rune::parser::parse_source;
use rune::warnings::collect_warnings;

#[test]
fn warns_about_unused_functions() {
    let program =
        parse_source("def helper() -> i64:\n    return 1\n\ndef main() -> i64:\n    return 0\n")
            .unwrap();
    let warnings = collect_warnings(&program);
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].message.contains("helper"));
}

#[test]
fn warns_about_unused_locals() {
    let program =
        parse_source("def main() -> i64:\n    let temp: i64 = 1\n    return 0\n").unwrap();
    let warnings = collect_warnings(&program);
    assert!(
        warnings
            .iter()
            .any(|warning| warning.message.contains("variable `temp` is never used"))
    );
}
