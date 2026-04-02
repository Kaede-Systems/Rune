use rune::lexer::{TokenKind, lex};

fn kinds(source: &str) -> Vec<TokenKind> {
    lex(source)
        .expect("lexing should succeed")
        .into_iter()
        .map(|token| token.kind)
        .collect()
}

#[test]
fn lexes_function_signature() {
    let tokens = kinds("def add(a: i64, b: i64) -> i64:\n    return a + b\n");
    assert_eq!(
        tokens,
        vec![
            TokenKind::Def,
            TokenKind::Identifier("add".into()),
            TokenKind::LParen,
            TokenKind::Identifier("a".into()),
            TokenKind::Colon,
            TokenKind::Identifier("i64".into()),
            TokenKind::Comma,
            TokenKind::Identifier("b".into()),
            TokenKind::Colon,
            TokenKind::Identifier("i64".into()),
            TokenKind::RParen,
            TokenKind::Arrow,
            TokenKind::Identifier("i64".into()),
            TokenKind::Colon,
            TokenKind::Newline,
            TokenKind::Indent,
            TokenKind::Return,
            TokenKind::Identifier("a".into()),
            TokenKind::Plus,
            TokenKind::Identifier("b".into()),
            TokenKind::Newline,
            TokenKind::Dedent,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn lexes_async_and_raise_keywords() {
    let tokens = kinds(
        "async def main() -> i32 raises IoError:\n    let name = await input()\n    raise IoError(\"bad\")\n",
    );
    assert!(tokens.contains(&TokenKind::Async));
    assert!(tokens.contains(&TokenKind::Await));
    assert!(tokens.contains(&TokenKind::Raise));
    assert!(tokens.contains(&TokenKind::Raises));
}

#[test]
fn emits_nested_indent_and_dedent() {
    let tokens = kinds("if true:\n    while false:\n        panic(\"no\")\n    return\n");
    let indent_count = tokens
        .iter()
        .filter(|kind| **kind == TokenKind::Indent)
        .count();
    let dedent_count = tokens
        .iter()
        .filter(|kind| **kind == TokenKind::Dedent)
        .count();
    assert_eq!(indent_count, 2);
    assert_eq!(dedent_count, 2);
}

#[test]
fn rejects_tabs() {
    let error = lex("def main():\n\treturn 0\n").expect_err("tabs must fail");
    assert!(error.message.contains("tabs"));
}

#[test]
fn rejects_inconsistent_indentation() {
    let error = lex("if true:\n    return 1\n  return 2\n")
        .expect_err("inconsistent indentation must fail");
    assert!(error.message.contains("inconsistent indentation"));
}

#[test]
fn skips_comments() {
    let tokens = kinds(
        "# file comment\n\ndef main() -> i32: # signature comment\n    # inside block\n    return 0\n",
    );
    assert!(tokens.contains(&TokenKind::Def));
    assert!(tokens.contains(&TokenKind::Return));
}

#[test]
fn skips_block_comments() {
    let tokens = kinds(
        "/* file block\ncomment */\ndef main() -> i32:\n    let value = 1 /* inline */ + 2\n    return value\n",
    );
    assert!(tokens.contains(&TokenKind::Def));
    assert!(tokens.contains(&TokenKind::Return));
    assert!(tokens.contains(&TokenKind::Plus));
}

#[test]
fn rejects_unterminated_block_comment() {
    let error = lex("def main() -> i32:\n    /* never ends\n    return 0\n")
        .expect_err("unterminated block comments must fail");
    assert!(error.message.contains("unterminated block comment"));
}

#[test]
fn lexes_fstrings() {
    let tokens = kinds("def main() -> unit:\n    println(f\"hello {42}\")\n");
    assert!(tokens.contains(&TokenKind::FString("hello {42}".into())));
}
