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
                  | return_stmt
                  | if_stmt
                  | while_stmt
                  | raise_stmt
                  | panic_stmt
                  | expr_stmt

let_stmt        ::= "let" ident [ ":" type_ref ] "=" expr NEWLINE
assign_stmt     ::= ident "=" expr NEWLINE
return_stmt     ::= "return" [ expr ] NEWLINE
if_stmt         ::= "if" expr ":" NEWLINE block
                    { "elif" expr ":" NEWLINE block }
                    [ "else" ":" NEWLINE block ]
while_stmt      ::= "while" expr ":" NEWLINE block
raise_stmt      ::= "raise" expr NEWLINE
panic_stmt      ::= "panic" expr NEWLINE
expr_stmt       ::= expr NEWLINE
```

## Expressions

```ebnf
expr            ::= or_expr
or_expr         ::= and_expr { "or" and_expr }
and_expr        ::= comparison { "and" comparison }
comparison      ::= additive { comp_op additive }
comp_op         ::= "==" | "!=" | ">" | ">=" | "<" | "<="
additive        ::= multiplicative { ("+" | "-") multiplicative }
multiplicative  ::= unary { ("*" | "/" | "%") unary }
unary           ::= "await" unary
                  | "-" unary
                  | "not" unary
                  | postfix
postfix         ::= primary { call_suffix | field_suffix }
call_suffix     ::= "(" [ call_args ] ")"
field_suffix    ::= "." ident
call_args       ::= call_arg { "," call_arg }
call_arg        ::= ident "=" expr | expr
primary         ::= ident | integer | string | "true" | "false" | "(" expr ")"
```

## Current Notes

- `await` parses, but async native backend support is still incomplete.
- `extern def` declarations are implemented for bodyless native C FFI declarations.
- `try` / `except` tokens exist lexically but are not parser-level language constructs yet.
- Struct/class declarations, constructor calls, and field reads are implemented for the current static native slice.
- Class methods declared inside the class body are implemented for the semantic checker, native executable path, and LLVM executable path.
- Current struct limitations:
  - struct values must be stored in explicitly typed locals like `let point: Point = ...`
  - struct parameters are supported for user functions in native codegen
  - struct return values are not yet supported in native codegen
  - `impl`, inheritance, traits/ABC, and dynamic dispatch are not implemented yet
- `raise` and `panic` both exist; `panic` is natively executable, and `raise` has a native runtime path for direct constructor/message forms.
