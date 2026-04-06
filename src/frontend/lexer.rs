use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    And,
    Assert,
    Async,
    Await,
    Break,
    Class,
    Continue,
    Def,
    Elif,
    Else,
    Exception,
    Except,
    Extern,
    For,
    From,
    If,
    Impl,
    Import,
    In,
    Let,
    Match,
    Case,
    Not,
    Or,
    Panic,
    Raise,
    Raises,
    Return,
    Struct,
    Trait,
    Try,
    Unsafe,
    While,
    True,
    False,
    Identifier(String),
    Integer(String),
    String(String),
    FString(String),
    Newline,
    Indent,
    Dedent,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Dot,
    Colon,
    Plus,
    Minus,
    Percent,
    Star,
    Slash,
    Equal,
    EqualEqual,
    NotEqual,
    Greater,
    GreaterEqual,
    Less,
    LessEqual,
    Ampersand,
    Pipe,
    Caret,
    Tilde,
    ShiftLeft,
    ShiftRight,
    PlusEqual,
    MinusEqual,
    StarEqual,
    SlashEqual,
    PercentEqual,
    AmpersandEqual,
    PipeEqual,
    CaretEqual,
    ShiftLeftEqual,
    ShiftRightEqual,
    Arrow,
    Eof,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexError {
    pub message: String,
    pub span: Span,
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at line {}, column {}",
            self.message, self.span.line, self.span.column
        )
    }
}

impl std::error::Error for LexError {}

pub fn lex(source: &str) -> Result<Vec<Token>, LexError> {
    let stripped = strip_block_comments(source)?;
    let mut lexer = Lexer::new(&stripped);
    lexer.lex_all()
}

fn strip_block_comments(source: &str) -> Result<String, LexError> {
    let mut out = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    let mut line = 1usize;
    let mut column = 1usize;
    let mut comment_start = None;
    let mut in_string = false;
    let mut escaping = false;

    while let Some(ch) = chars.next() {
        let next = chars.peek().copied();

        if let Some((start_line, start_column)) = comment_start {
            if ch == '*' && next == Some('/') {
                chars.next();
                column += 2;
                comment_start = None;
                continue;
            }
            if ch == '\n' {
                out.push('\n');
                line += 1;
                column = 1;
            } else {
                column += 1;
            }
            let _ = (start_line, start_column);
            continue;
        }

        if in_string {
            out.push(ch);
            if escaping {
                escaping = false;
            } else if ch == '\\' {
                escaping = true;
            } else if ch == '"' {
                in_string = false;
            }
            if ch == '\n' {
                line += 1;
                column = 1;
            } else {
                column += 1;
            }
            continue;
        }

        if ch == '"' {
            in_string = true;
            out.push(ch);
            column += 1;
            continue;
        }

        if ch == '/' && next == Some('*') {
            chars.next();
            comment_start = Some((line, column));
            column += 2;
            continue;
        }

        out.push(ch);
        if ch == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }

    if let Some((start_line, start_column)) = comment_start {
        return Err(LexError {
            message: "unterminated block comment".to_string(),
            span: Span {
                line: start_line,
                column: start_column,
            },
        });
    }

    Ok(out)
}

struct Lexer<'a> {
    lines: std::str::Lines<'a>,
    line_number: usize,
    tokens: Vec<Token>,
    indent_stack: Vec<usize>,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            lines: source.lines(),
            line_number: 0,
            tokens: Vec::new(),
            indent_stack: vec![0],
        }
    }

    fn lex_all(&mut self) -> Result<Vec<Token>, LexError> {
        while let Some(line) = self.lines.next() {
            self.line_number += 1;
            self.lex_line(line)?;
        }

        while self.indent_stack.len() > 1 {
            self.indent_stack.pop();
            self.push(TokenKind::Dedent, 1);
        }

        self.push(TokenKind::Eof, 1);
        Ok(std::mem::take(&mut self.tokens))
    }

    fn lex_line(&mut self, raw_line: &str) -> Result<(), LexError> {
        if raw_line.contains('\t') {
            return Err(self.error("tabs are not allowed for indentation", 1));
        }

        let indent = raw_line.chars().take_while(|&ch| ch == ' ').count();
        let content = &raw_line[indent..];
        let trimmed = content.trim();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            return Ok(());
        }

        self.handle_indent(indent)?;

        let mut chars = content.char_indices().peekable();
        while let Some((offset, ch)) = chars.next() {
            let column = indent + offset + 1;
            match ch {
                ' ' => {}
                '#' => break,
                'f' => {
                    if let Some((_, '"')) = chars.peek().copied() {
                        chars.next();
                        let start_column = column;
                        let value = self.lex_string_contents(&mut chars, start_column)?;
                        self.push(TokenKind::FString(value), start_column);
                    } else {
                        let start = offset;
                        let mut end = offset + ch.len_utf8();
                        while let Some((next_offset, next_ch)) = chars.peek().copied() {
                            if next_ch.is_ascii_alphanumeric() || next_ch == '_' {
                                chars.next();
                                end = next_offset + next_ch.len_utf8();
                            } else {
                                break;
                            }
                        }
                        let text = &content[start..end];
                        let kind = keyword_or_ident(text);
                        self.push(kind, column);
                    }
                }
                'a'..='z' | 'A'..='Z' | '_' => {
                    let start = offset;
                    let mut end = offset + ch.len_utf8();
                    while let Some((next_offset, next_ch)) = chars.peek().copied() {
                        if next_ch.is_ascii_alphanumeric() || next_ch == '_' {
                            chars.next();
                            end = next_offset + next_ch.len_utf8();
                        } else {
                            break;
                        }
                    }
                    let text = &content[start..end];
                    let kind = keyword_or_ident(text);
                    self.push(kind, column);
                }
                '0'..='9' => {
                    let start = offset;
                    let mut end = offset + ch.len_utf8();
                    // Detect 0x / 0o / 0b prefixes
                    if ch == '0' {
                        if let Some((_, prefix_ch)) = chars.peek().copied() {
                            match prefix_ch {
                                'x' | 'X' => {
                                    chars.next();
                                    end += prefix_ch.len_utf8();
                                    while let Some((next_offset, next_ch)) = chars.peek().copied() {
                                        if next_ch.is_ascii_hexdigit() || next_ch == '_' {
                                            chars.next();
                                            end = next_offset + next_ch.len_utf8();
                                        } else {
                                            break;
                                        }
                                    }
                                    self.push(TokenKind::Integer(content[start..end].to_string()), column);
                                    continue;
                                }
                                'o' | 'O' => {
                                    chars.next();
                                    end += prefix_ch.len_utf8();
                                    while let Some((next_offset, next_ch)) = chars.peek().copied() {
                                        if matches!(next_ch, '0'..='7') || next_ch == '_' {
                                            chars.next();
                                            end = next_offset + next_ch.len_utf8();
                                        } else {
                                            break;
                                        }
                                    }
                                    self.push(TokenKind::Integer(content[start..end].to_string()), column);
                                    continue;
                                }
                                'b' | 'B' => {
                                    chars.next();
                                    end += prefix_ch.len_utf8();
                                    while let Some((next_offset, next_ch)) = chars.peek().copied() {
                                        if matches!(next_ch, '0' | '1') || next_ch == '_' {
                                            chars.next();
                                            end = next_offset + next_ch.len_utf8();
                                        } else {
                                            break;
                                        }
                                    }
                                    self.push(TokenKind::Integer(content[start..end].to_string()), column);
                                    continue;
                                }
                                _ => {}
                            }
                        }
                    }
                    while let Some((next_offset, next_ch)) = chars.peek().copied() {
                        if next_ch.is_ascii_digit() || next_ch == '_' {
                            chars.next();
                            end = next_offset + next_ch.len_utf8();
                        } else {
                            break;
                        }
                    }
                    self.push(TokenKind::Integer(content[start..end].to_string()), column);
                }
                '"' => {
                    let start_column = column;
                    let value = self.lex_string_contents(&mut chars, start_column)?;
                    self.push(TokenKind::String(value), start_column);
                }
                '(' => self.push(TokenKind::LParen, column),
                ')' => self.push(TokenKind::RParen, column),
                '[' => self.push(TokenKind::LBracket, column),
                ']' => self.push(TokenKind::RBracket, column),
                ',' => self.push(TokenKind::Comma, column),
                '.' => self.push(TokenKind::Dot, column),
                ':' => self.push(TokenKind::Colon, column),
                '+' => {
                    if let Some((_, '=')) = chars.peek().copied() {
                        chars.next();
                        self.push(TokenKind::PlusEqual, column);
                    } else {
                        self.push(TokenKind::Plus, column);
                    }
                }
                '%' => {
                    if let Some((_, '=')) = chars.peek().copied() {
                        chars.next();
                        self.push(TokenKind::PercentEqual, column);
                    } else {
                        self.push(TokenKind::Percent, column);
                    }
                }
                '*' => {
                    if let Some((_, '=')) = chars.peek().copied() {
                        chars.next();
                        self.push(TokenKind::StarEqual, column);
                    } else {
                        self.push(TokenKind::Star, column);
                    }
                }
                '/' => {
                    if let Some((_, '=')) = chars.peek().copied() {
                        chars.next();
                        self.push(TokenKind::SlashEqual, column);
                    } else {
                        self.push(TokenKind::Slash, column);
                    }
                }
                '&' => {
                    if let Some((_, '=')) = chars.peek().copied() {
                        chars.next();
                        self.push(TokenKind::AmpersandEqual, column);
                    } else {
                        self.push(TokenKind::Ampersand, column);
                    }
                }
                '|' => {
                    if let Some((_, '=')) = chars.peek().copied() {
                        chars.next();
                        self.push(TokenKind::PipeEqual, column);
                    } else {
                        self.push(TokenKind::Pipe, column);
                    }
                }
                '^' => {
                    if let Some((_, '=')) = chars.peek().copied() {
                        chars.next();
                        self.push(TokenKind::CaretEqual, column);
                    } else {
                        self.push(TokenKind::Caret, column);
                    }
                }
                '~' => self.push(TokenKind::Tilde, column),
                '-' => {
                    if let Some((_, '>')) = chars.peek().copied() {
                        chars.next();
                        self.push(TokenKind::Arrow, column);
                    } else if let Some((_, '=')) = chars.peek().copied() {
                        chars.next();
                        self.push(TokenKind::MinusEqual, column);
                    } else {
                        self.push(TokenKind::Minus, column);
                    }
                }
                '=' => {
                    if let Some((_, '=')) = chars.peek().copied() {
                        chars.next();
                        self.push(TokenKind::EqualEqual, column);
                    } else {
                        self.push(TokenKind::Equal, column);
                    }
                }
                '!' => {
                    if let Some((_, '=')) = chars.peek().copied() {
                        chars.next();
                        self.push(TokenKind::NotEqual, column);
                    } else {
                        return Err(self.error("unexpected character '!'", column));
                    }
                }
                '>' => {
                    if let Some((_, '=')) = chars.peek().copied() {
                        chars.next();
                        self.push(TokenKind::GreaterEqual, column);
                    } else if let Some((_, '>')) = chars.peek().copied() {
                        chars.next();
                        if let Some((_, '=')) = chars.peek().copied() {
                            chars.next();
                            self.push(TokenKind::ShiftRightEqual, column);
                        } else {
                            self.push(TokenKind::ShiftRight, column);
                        }
                    } else {
                        self.push(TokenKind::Greater, column);
                    }
                }
                '<' => {
                    if let Some((_, '=')) = chars.peek().copied() {
                        chars.next();
                        self.push(TokenKind::LessEqual, column);
                    } else if let Some((_, '<')) = chars.peek().copied() {
                        chars.next();
                        if let Some((_, '=')) = chars.peek().copied() {
                            chars.next();
                            self.push(TokenKind::ShiftLeftEqual, column);
                        } else {
                            self.push(TokenKind::ShiftLeft, column);
                        }
                    } else {
                        self.push(TokenKind::Less, column);
                    }
                }
                other => {
                    return Err(self.error(&format!("unexpected character '{}'", other), column));
                }
            }
        }

        self.push(TokenKind::Newline, content.len() + indent + 1);
        Ok(())
    }

    fn handle_indent(&mut self, indent: usize) -> Result<(), LexError> {
        let current = *self
            .indent_stack
            .last()
            .expect("indent stack is never empty");
        if indent > current {
            self.indent_stack.push(indent);
            self.push(TokenKind::Indent, 1);
            return Ok(());
        }

        if indent == current {
            return Ok(());
        }

        while indent
            < *self
                .indent_stack
                .last()
                .expect("indent stack is never empty")
        {
            self.indent_stack.pop();
            self.push(TokenKind::Dedent, 1);
        }

        if indent
            != *self
                .indent_stack
                .last()
                .expect("indent stack is never empty")
        {
            return Err(self.error("inconsistent indentation", 1));
        }

        Ok(())
    }

    fn push(&mut self, kind: TokenKind, column: usize) {
        self.tokens.push(Token {
            kind,
            span: Span {
                line: self.line_number.max(1),
                column,
            },
        });
    }

    fn error(&self, message: &str, column: usize) -> LexError {
        LexError {
            message: message.to_string(),
            span: Span {
                line: self.line_number.max(1),
                column,
            },
        }
    }

    fn lex_string_contents<I>(
        &self,
        chars: &mut std::iter::Peekable<I>,
        start_column: usize,
    ) -> Result<String, LexError>
    where
        I: Iterator<Item = (usize, char)>,
    {
        let mut value = String::new();
        let mut closed = false;

        while let Some((_, next_ch)) = chars.next() {
            match next_ch {
                '"' => {
                    closed = true;
                    break;
                }
                '\\' => {
                    let Some((_, escaped)) = chars.next() else {
                        return Err(self.error("unterminated escape sequence", start_column));
                    };
                    let translated = match escaped {
                        'n' => '\n',
                        'r' => '\r',
                        't' => '\t',
                        '\\' => '\\',
                        '"' => '"',
                        other => {
                            return Err(self.error(
                                &format!("unsupported escape sequence \\{}", other),
                                start_column,
                            ));
                        }
                    };
                    value.push(translated);
                }
                other => value.push(other),
            }
        }

        if !closed {
            return Err(self.error("unterminated string literal", start_column));
        }

        Ok(value)
    }
}

fn keyword_or_ident(text: &str) -> TokenKind {
    match text {
        "and" => TokenKind::And,
        "assert" => TokenKind::Assert,
        "async" => TokenKind::Async,
        "await" => TokenKind::Await,
        "break" => TokenKind::Break,
        "class" => TokenKind::Class,
        "continue" => TokenKind::Continue,
        "def" => TokenKind::Def,
        "elif" => TokenKind::Elif,
        "else" => TokenKind::Else,
        "exception" => TokenKind::Exception,
        "except" => TokenKind::Except,
        "extern" => TokenKind::Extern,
        "for" => TokenKind::For,
        "from" => TokenKind::From,
        "if" => TokenKind::If,
        "impl" => TokenKind::Impl,
        "import" => TokenKind::Import,
        "in" => TokenKind::In,
        "let" => TokenKind::Let,
        "match" => TokenKind::Match,
        "case" => TokenKind::Case,
        "not" => TokenKind::Not,
        "or" => TokenKind::Or,
        "panic" => TokenKind::Panic,
        "raise" => TokenKind::Raise,
        "raises" => TokenKind::Raises,
        "return" => TokenKind::Return,
        "struct" => TokenKind::Struct,
        "trait" => TokenKind::Trait,
        "try" => TokenKind::Try,
        "unsafe" => TokenKind::Unsafe,
        "while" => TokenKind::While,
        "true" => TokenKind::True,
        "false" => TokenKind::False,
        _ => TokenKind::Identifier(text.to_string()),
    }
}
