# Rune Syntax

This file describes the syntax and standard library surface that currently exists in Rune.

It is intentionally practical and reflects the repository today.

## Style

Rune uses Python-inspired surface syntax:

- `def` and `async def`
- indentation for blocks
- `if`, `elif`, `else`, `while`
- keyword arguments
- `and`, `or`, `not`
- `raise` and `panic`
- `struct`
- `extern def`

Example:

```rune
def main() -> i32:
    let value = 10
    value = value + 2

    if value > 5 and not false:
        println("ok")

    return 0
```

## Variables

Typed local:

```rune
let count: i32 = 10
```

Untyped local:

```rune
let value = 10
```

Untyped locals currently default to `dynamic`.

## Functions

```rune
def add(a: i32, b: i32) -> i32:
    return a + b
```

Untyped parameters default to `dynamic`:

```rune
def echo(value) -> unit:
    println(value)
    return
```

Async syntax currently exists:

```rune
async def run() -> i32:
    let text = await input()
    println(text)
    return 0
```

Native C FFI declarations currently use bodyless `extern def`:

```rune
extern def add_from_c(a: i32, b: i32) -> i32

def main() -> i32:
    return add_from_c(20, 22)
```

Current C FFI import scope:

- native build paths only
- currently supported outbound ABI types: `bool`, `i32`, `i64`, `String`, `unit`
- explicit linker inputs via `rune build --link-lib`, `--link-search`, or `--link-arg`
- automatic C source compilation for executable builds via `rune build --link-c-source file.c`
- Rune shared and static library builds now also emit a matching C header next to the library artifact
- C consumer flow is verified on Windows against the generated header and Rune static libraries
- you can also emit the header directly with `rune emit-c-header file.rn -o file.h`

Current outbound `String` FFI rule:

- Rune `String` arguments are passed to C as UTF-8 null-terminated `const char*`
- C `const char*` return values are converted back into Rune `String`

## Control Flow

```rune
if cond:
    println("yes")
elif other:
    println("maybe")
else:
    println("no")
```

```rune
while count > 0:
    count = count - 1
```

Dynamic truthiness is currently supported in native code paths for conditions.

## Structs

Concrete struct declarations are now implemented for the current static slice:

```rune
struct Point:
    x: i32
    y: i32

def main() -> i32:
    let point: Point = Point(x=20, y=22)
    println(point.x)
    println(point.y)
    return point.x + point.y
```

Current implemented struct rules:

- construction uses keyword arguments
- field reads use `value.field`
- struct locals must currently be explicitly typed
- struct parameters are supported for user functions
- struct values are stack-backed in the native backend

Current struct limitations:

- struct return values are not yet supported in native codegen
- `impl`, methods, inheritance, traits, and ABCs are not implemented yet

## Operators

Current implemented operator surface:

- arithmetic: `+`, `-`, `*`, `/`, `%`
- comparison: `==`, `!=`, `>`, `>=`, `<`, `<=`
- boolean: `and`, `or`, `not`

Current dynamic behavior:

- dynamic `+` supports numeric addition and string concatenation
- dynamic `-`, `*`, `/`, `%` support numeric-like values
- dynamic comparisons are runtime-dispatched
- dynamic truthiness is runtime-dispatched

## Calls

Positional:

```rune
add(1, 2)
```

Keyword:

```rune
connect(host="127.0.0.1", port=8080)
```

Mixed:

```rune
add(10, rhs=32)
```

## Imports

Local imports:

```rune
import math
from math import add
```

Relative imports:

```rune
from .math import add
from ..shared.util import helper
```

Stdlib-style imports currently use top-level module names:

```rune
from time import unix_now
from system import pid
from env import has
from network import tcp_connect
```

## Exceptions

Declared exceptions:

```rune
exception NetworkError
exception ParseError
```

Function declaration with `raises`:

```rune
def load() -> unit raises NetworkError:
    raise NetworkError("failed")
```

Current status:

- exception declarations parse
- `raises` is semantically checked
- `panic` is natively executable
- `raise` lowers natively for direct exception-constructor or string forms
- full `try` / `except` propagation is not implemented yet

## Panic

```rune
panic("fatal error")
```

Current native runtime behavior:

- prints a panic message to stderr
- includes function/line context
- exits with code `101`

## Builtins

Language-level builtins currently recognized:

- `print(...)`
- `println(...)`
- `input()`
- `str(value)`
- `int(value)`
- `panic(...)`

## Current Standard Library Surface

These top-level stdlib modules currently exist in [`stdlib/`](C:\Users\kaededevkentohinode\KUROX\stdlib):

- `time`
- `system`
- `env`
- `network`
- `fs`
- `terminal`
- `audio`
- `io`

Current exported functions:

`time`
- `unix_now() -> i64`
- `monotonic_ms() -> i64`
- `sleep_ms(ms: i64) -> unit`
- `sleep(seconds: i64) -> unit`
- `sleep_until(deadline_ms: i64) -> unit`

`system`
- `pid() -> i32`
- `cpu_count() -> i32`
- `exit(code: i32) -> unit`
- `quit(code: i32) -> unit`
- `exit_success() -> unit`
- `exit_failure() -> unit`

`env`
- `has(name: String) -> bool`
- `get_i32(name: String, default: i32) -> i32`
- `get_bool(name: String, default: bool) -> bool`
- `arg_count() -> i32`
- `get_i32_or_zero(name: String) -> i32`
- `get_bool_or_false(name: String) -> bool`
- `get_bool_or_true(name: String) -> bool`

`network`
- `tcp_connect(host: String, port: i32) -> bool`
- `tcp_connect_timeout(host: String, port: i32, timeout_ms: i32) -> bool`
- `tcp_probe(host: String, port: i32) -> bool`
- `tcp_probe_timeout(host: String, port: i32, timeout_ms: i32) -> bool`

`fs`
- `exists(path: String) -> bool`
- `read_string(path: String) -> String`
- `read_text(path: String) -> String`
- `write_string(path: String, content: String) -> bool`
- `write_text(path: String, content: String) -> bool`

`terminal`
- `clear() -> unit`
- `move_to(row: i32, col: i32) -> unit`
- `hide_cursor() -> unit`
- `show_cursor() -> unit`
- `set_title(title: String) -> unit`
- `clear_screen() -> unit`
- `home() -> unit`
- `clear_and_home() -> unit`
- `hide() -> unit`
- `show() -> unit`

`audio`
- `bell() -> bool`
- `beep() -> bool`

`io`
- `write(value) -> unit`
- `writeln(value) -> unit`
- `error(value) -> unit`
- `errorln(value) -> unit`
- `flush_out() -> unit`
- `flush_err() -> unit`
- `read_line() -> String`

## Native Backend Notes

Currently working native slices include:

- integer arithmetic
- string conversion and concatenation
- dynamic values and reassignment
- dynamic operators and comparisons
- stack-backed struct locals with field reads
- printing
- imports
- diagnostics
- panic

Still not fully complete for native release scope:

- full async runtime
- `try` / `except`
- complete OOP/ABC surface
- full HTTP / WS / server APIs
- full `raise` / exception propagation model
