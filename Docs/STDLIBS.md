# Rune Standard Library

This document lists the Rune standard library modules that are implemented today.

These modules live in [`stdlib/`](/C:/Users/kaededevkentohinode/KUROX/stdlib).

## `io`

```rune
from io import write, writeln, error, errorln, flush_out, flush_err, read_line
```

Exports:

- `write(value) -> unit`
- `writeln(value) -> unit`
- `error(value) -> unit`
- `errorln(value) -> unit`
- `flush_out() -> unit`
- `flush_err() -> unit`
- `read_line() -> String`

Current implemented IO scope:

- stdout writes
- stderr writes
- explicit stdout/stderr flush
- line input

## `time`

```rune
from time import unix_now, monotonic_ms, sleep_ms, sleep, sleep_until
```

Exports:

- `unix_now() -> i64`
- `monotonic_ms() -> i64`
- `sleep_ms(ms: i64) -> unit`
- `sleep(seconds: i64) -> unit`
- `sleep_until(deadline_ms: i64) -> unit`

## `system`

```rune
from system import pid, cpu_count, exit, quit, exit_success, exit_failure
```

Exports:

- `pid() -> i32`
- `cpu_count() -> i32`
- `exit(code: i32) -> unit`
- `quit(code: i32) -> unit`
- `exit_success() -> unit`
- `exit_failure() -> unit`

## `env`

```rune
from env import has, get_i32, get_bool, arg_count
from env import get_i32_or_zero, get_bool_or_false, get_bool_or_true
```

Exports:

- `has(name: String) -> bool`
- `get_i32(name: String, default: i32) -> i32`
- `get_bool(name: String, default: bool) -> bool`
- `arg_count() -> i32`
- `get_i32_or_zero(name: String) -> i32`
- `get_bool_or_false(name: String) -> bool`
- `get_bool_or_true(name: String) -> bool`

## `network`

```rune
from network import tcp_connect, tcp_connect_timeout
from network import tcp_probe, tcp_probe_timeout
```

Exports:

- `tcp_connect(host: String, port: i32) -> bool`
- `tcp_connect_timeout(host: String, port: i32, timeout_ms: i32) -> bool`
- `tcp_probe(host: String, port: i32) -> bool`
- `tcp_probe_timeout(host: String, port: i32, timeout_ms: i32) -> bool`

Current implemented network scope:

- TCP client connectivity probes
- timeout-aware TCP probe

Not implemented in this module yet:

- TCP server sockets
- UDP
- HTTP
- WebSocket

## `fs`

```rune
from fs import exists, read_string, read_text, write_string, write_text
```

Exports:

- `exists(path: String) -> bool`
- `read_string(path: String) -> String`
- `read_text(path: String) -> String`
- `write_string(path: String, content: String) -> bool`
- `write_text(path: String, content: String) -> bool`

Current implemented filesystem scope:

- existence checks
- reading UTF-8 text files
- writing UTF-8 text files

Not implemented in this module yet:

- directory walking
- file deletion
- rename/copy

## `terminal`

```rune
from terminal import clear, move_to, hide_cursor, show_cursor, set_title
from terminal import clear_screen, home, clear_and_home, hide, show
```

Exports:

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

## `audio`

```rune
from audio import bell, beep
```

Exports:

- `bell() -> bool`
- `beep() -> bool`

Current implemented audio scope:

- terminal bell / beep signal
