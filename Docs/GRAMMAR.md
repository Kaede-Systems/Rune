# Rune Grammar

This document describes the current implemented Rune grammar in the repository today.

It reflects the real parser, not the long-term design vision.

## Lexical Notes

- Files use indentation-sensitive blocks.
- Tabs are rejected for indentation.
- Comments start with `#` and run to end of line.
- Strings use double quotes.
- Blocks are represented internally with `INDENT` and `DEDENT`.

## Top-Level

```ebnf
program         ::= { item }
item            ::= import_decl | exception_decl | struct_decl | function_decl
```

## Imports

```ebnf
import_decl     ::= "import" module_path NEWLINE
                  | "from" module_path "import" ident { "," ident } NEWLINE

module_path     ::= { "." } ident { "." ident }
```

## Exceptions

```ebnf
exception_decl  ::= "exception" ident NEWLINE
```

## Classes And Structs

```ebnf
struct_decl     ::= ( "struct" | "class" ) ident ":" NEWLINE INDENT { struct_member } DEDENT
struct_member   ::= struct_field | method_decl
struct_field    ::= ident ":" type_ref NEWLINE
method_decl     ::= [ "extern" ] [ "async" ] "def" ident "(" [ param_list ] ")" [ return_ann ] [ raises_ann ] ":" NEWLINE block
```

## Functions

```ebnf
function_decl   ::= [ "extern" ] [ "async" ] "def" ident "(" [ param_list ] ")" [ return_ann ] [ raises_ann ] function_tail
function_tail   ::= NEWLINE
                  | ":" NEWLINE block
param_list      ::= param { "," param }
param           ::= ident [ ":" type_ref ]
return_ann      ::= "->" type_ref
raises_ann      ::= "raises" type_ref
type_ref        ::= ident { "." ident }
```

Untyped parameters currently default to `dynamic`.

## Blocks

```ebnf
block           ::= INDENT { stmt } DEDENT
```

## Statements

```ebnf
stmt            ::= let_stmt
                  | assign_stmt
                  | augmented_assign_stmt
                  | field_assign_stmt
                  | return_stmt
                  | if_stmt
                  | while_stmt
                  | for_stmt
                  | match_stmt
                  | break_stmt
                  | continue_stmt
                  | raise_stmt
                  | panic_stmt
                  | assert_stmt
                  | expr_stmt

let_stmt            ::= "let" ident [ ":" type_ref ] "=" expr NEWLINE
assign_stmt         ::= ident "=" expr NEWLINE
augmented_assign    ::= "+=" | "-=" | "*=" | "/=" | "%=" | "&=" | "|=" | "^=" | "<<=" | ">>="
augmented_assign_stmt ::= ident augmented_assign expr NEWLINE
                        | ident { "." ident } augmented_assign expr NEWLINE
field_assign_stmt   ::= ident { "." ident } "=" expr NEWLINE
return_stmt         ::= "return" [ expr ] NEWLINE
if_stmt             ::= "if" expr ":" NEWLINE block
                        { "elif" expr ":" NEWLINE block }
                        [ "else" ":" NEWLINE block ]
while_stmt          ::= "while" expr ":" NEWLINE block
for_stmt            ::= "for" ident "in" range_call ":" NEWLINE block
range_call          ::= "range" "(" expr ")"
                      | "range" "(" expr "," expr ")"
                      | "range" "(" expr "," expr "," expr ")"
match_stmt          ::= "match" expr ":" NEWLINE INDENT { match_arm } DEDENT
match_arm           ::= "case" ( integer | "-" integer | string | "_" ) ":" NEWLINE block
break_stmt          ::= "break" NEWLINE
continue_stmt       ::= "continue" NEWLINE
raise_stmt          ::= "raise" expr NEWLINE
panic_stmt          ::= "panic" expr NEWLINE
assert_stmt         ::= "assert" expr [ "," expr ] NEWLINE
expr_stmt           ::= expr NEWLINE
```

## Expressions

```ebnf
expr            ::= or_expr
or_expr         ::= and_expr { "or" and_expr }
and_expr        ::= not_expr { "and" not_expr }
not_expr        ::= "not" not_expr | comparison
comparison      ::= bitwise_or { comp_op bitwise_or }
comp_op         ::= "==" | "!=" | ">" | ">=" | "<" | "<="
bitwise_or      ::= bitwise_xor { "|" bitwise_xor }
bitwise_xor     ::= bitwise_and { "^" bitwise_and }
bitwise_and     ::= shift { "&" shift }
shift           ::= additive { ( "<<" | ">>" ) additive }
additive        ::= multiplicative { ( "+" | "-" ) multiplicative }
multiplicative  ::= unary { ( "*" | "/" | "%" ) unary }
unary           ::= "await" unary
                  | "-" unary
                  | "not" unary
                  | "~" unary
                  | postfix
postfix         ::= primary { call_suffix | field_suffix }
call_suffix     ::= "(" [ call_args ] ")"
field_suffix    ::= "." ident
call_args       ::= call_arg { "," call_arg }
call_arg        ::= ident "=" expr | expr
primary         ::= ident | integer | string | bool_literal | fstring | "(" expr ")"
bool_literal    ::= "true" | "false"
integer         ::= decimal_int | "0x" hex_digits | "0o" oct_digits | "0b" bin_digits
fstring         ::= "f\"" { fstring_literal | "{" expr "}" | "{{" | "}}" } "\""
```

## String Methods

String method calls are resolved via the `postfix` rule when the receiver has type `String`:

```ebnf
string_method  ::= expr "." string_method_name "(" [ call_args ] ")"
string_method_name ::= "len" | "upper" | "lower" | "strip" | "trim_start" | "trim_end"
                     | "repeat" | "contains" | "starts_with" | "ends_with"
                     | "find" | "replace" | "slice"
```

Return types:
- `len` → `i64`
- `upper`, `lower`, `strip`, `trim_start`, `trim_end`, `replace`, `repeat`, `slice` → `String`
- `contains`, `starts_with`, `ends_with` → `bool`
- `find` → `i64` (byte index of first occurrence, or `-1`)

## Current Notes

- `await` parses, but async native backend support is still incomplete.
- `extern def` declarations are implemented for bodyless native C FFI declarations.
- `try` / `except` tokens exist lexically but are not parser-level language constructs yet.
- `import module` plus `module.name(...)` namespace-qualified access is implemented.
- import aliases such as `import module as alias` are not implemented yet.
- `for` loops currently require `range(...)` as the iterable; iterating over strings or user-defined iterables is not yet implemented.
- `match` desugars to if/elif/else at parse time; patterns are limited to integer literals, string literals, and `_` wildcard.
- `assert expr` panics with a standard message; `assert expr, message` panics with the provided string.
- Augmented assignment operators (`+=`, `-=`, `*=`, `/=`, `%=`, `&=`, `|=`, `^=`, `<<=`, `>>=`) are implemented.
- Field assignment (`obj.field = value`, `obj.a.b = value`) is implemented.
- Bitwise operators (`&`, `|`, `^`, `~`, `<<`, `>>`) are implemented.
- Integer literals: decimal, `0x` hex, `0o` octal, `0b` binary are all supported.
- String methods: `len`, `upper`, `lower`, `strip`, `trim_start`, `trim_end`, `repeat`, `contains`, `starts_with`, `ends_with`, `find`, `replace`, `slice` are all implemented across all backends.
- Integer math builtins: `abs(x: i64) -> i64`, `min(a: i64, b: i64) -> i64`, `max(a: i64, b: i64) -> i64`, `clamp(x: i64, lo: i64, hi: i64) -> i64`, `pow(base: i64, exp: i64) -> i64` are implemented across all backends.
- Character builtins: `chr(n: i64) -> String` (codepoint to UTF-8 string), `ord(s: String) -> i64` (first codepoint of string) are implemented across all backends.
- Struct/class declarations, constructor calls, and field reads are implemented for the current static native slice.
- Class methods declared inside the class body are implemented for the semantic checker, native executable path, and LLVM executable path.
- Current struct limitations:
  - struct values must be stored in explicitly typed locals like `let point: Point = ...`
  - struct parameters are supported for user functions in native codegen
  - struct and class return values are supported in native, LLVM, and AVR executable builds
  - `impl`, inheritance, traits/ABC, and dynamic dispatch are not implemented yet
- `raise` and `panic` both exist; `panic` is natively executable, and `raise` has a native runtime path for direct constructor/message forms.
