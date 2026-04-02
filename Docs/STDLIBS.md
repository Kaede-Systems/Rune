# Rune Standard Library

This document lists the Rune standard library modules that are implemented today.

These modules live in [`stdlib/`](/C:/Users/kaededevkentohinode/KUROX/stdlib).

## `arduino`

```rune
from arduino import (
    pin_mode, digital_write, digital_read,
    analog_write, analog_read, analog_reference,
    pulse_in, shift_out, tone, no_tone,
    delay_ms, delay_us, millis, micros,
    read_line,
    mode_input, mode_output, mode_input_pullup, led_builtin,
    high, low,
    bit_order_lsb_first, bit_order_msb_first,
    analog_ref_default, analog_ref_internal, analog_ref_external,
    uart_begin, uart_available, uart_read_byte, uart_write_byte, uart_write,
)
```

Exports:

- `pin_mode(pin: i64, mode: i64) -> unit`
- `digital_write(pin: i64, value: bool) -> unit`
- `digital_read(pin: i64) -> bool`
- `analog_write(pin: i64, value: i64) -> unit`
- `analog_read(pin: i64) -> i64`
- `analog_reference(mode: i64) -> unit`
- `pulse_in(pin: i64, state: bool, timeout_us: i64) -> i64`
- `shift_out(data_pin: i64, clock_pin: i64, bit_order: i64, value: i64) -> unit`
- `tone(pin: i64, frequency_hz: i64, duration_ms: i64) -> unit`
- `no_tone(pin: i64) -> unit`
- `delay_ms(ms: i64) -> unit`
- `delay_us(us: i64) -> unit`
- `millis() -> i64`
- `micros() -> i64`
- `read_line() -> String`
- `mode_input() -> i64`
- `mode_output() -> i64`
- `mode_input_pullup() -> i64`
- `led_builtin() -> i64`
- `high() -> i64`
- `low() -> i64`
- `bit_order_lsb_first() -> i64`
- `bit_order_msb_first() -> i64`
- `analog_ref_default() -> i64`
- `analog_ref_internal() -> i64`
- `analog_ref_external() -> i64`
- `uart_begin(baud: i64) -> unit`
- `uart_available() -> i64`
- `uart_read_byte() -> i64`
- `uart_write_byte(value: i64) -> unit`
- `uart_write(text: String) -> unit`

Current implemented Arduino scope:

- packaged Uno-target stdlib resolution through `from arduino import ...`
- serial text output with normal Rune `print` and `println`
- serial line input with normal Rune `input()` and `read_line()`
- byte-oriented UART access with `uart_available`, `uart_read_byte`, and `uart_write_byte`
- board constants and pin/timing helpers
- PWM, pulse timing, tone generation, shift register output, and analog reference selection
- Arduino-style `setup()` / `loop()` entrypoints on the Uno target

Recommended usage:

- use `print`, `println`, and `input()` when you want the same high-level syntax as desktop Rune
- use `uart_*` only when you specifically need byte-level serial control on the board

Current Arduino limitations:

- this module is implemented for the current Uno embedded slice, not full Rune parity
- dynamic values and full class/OOP parity are not implemented on AVR yet

## `serial`

```rune
from serial import begin, open, is_open, close
from serial import available, read_byte, recv_line, write, write_line, send, send_line
```

Exports:

- `begin(baud: i64) -> unit`
- `open(port: String, baud: i64) -> bool`
- `is_open() -> bool`
- `close() -> unit`
- `available() -> i64`
- `read_byte() -> i64`
- `recv_line() -> String`
- `write(text: String) -> unit`
- `write_line(text: String) -> unit`
- `send(text: String) -> bool`
- `send_line(text: String) -> bool`

Current implemented serial scope:

- shared serial-facing Rune surface for embedded and host code
- on Arduino Uno:
  - `begin` lowers to `uart_begin`
  - `open` behaves like `begin`
  - `write` and `write_line` lower to UART writes
  - `send` and `send_line` lower to UART writes and report success
  - `recv_line` lowers to the normal embedded input surface
- on non-embedded targets:
  - `open` opens a host serial port such as `COM5`
  - `is_open`, `close`, `send`, `send_line`, and `recv_line` talk to the active host serial connection
  - `write` / `write_line` still lower to `print` / `println`

Current serial limitations:

- this is a single-active-connection text serial layer, not a full multi-port device API
- text line input stays on the normal Rune `input()` surface
- lower-level byte control remains in `arduino` via `uart_*`
- current host serial scope is for native host builds, not browser/WASM targets

## `json`

```rune
from json import parse, stringify, kind, is_null, len, get, index
from json import to_string, to_i64, to_bool
```

Exports:

- `parse(text: String) -> Json`
- `stringify(value: Json) -> String`
- `kind(value: Json) -> String`
- `is_null(value: Json) -> bool`
- `len(value: Json) -> i64`
- `get(value: Json, key: String) -> Json`
- `index(value: Json, at: i64) -> Json`
- `to_string(value: Json) -> String`
- `to_i64(value: Json) -> i64`
- `to_bool(value: Json) -> bool`

Current implemented JSON scope:

- parsing validated JSON text into a first-class `Json` value
- stringifying full JSON values
- object field lookup
- array indexing
- null checks
- container length
- scalar conversion to `String`, `i64`, and `bool`

Current JSON limitations:

- direct `Json == Json` and `Json != Json` are implemented as structural comparisons
- JSON values are runtime-backed, not native arrays/maps in the language type system yet

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
from system import pid, cpu_count, platform, arch, target, board, is_embedded, is_wasm, exit, quit, exit_success, exit_failure

from sys import platform, arch, target, board, is_embedded, is_wasm
```

Exports:

- `pid() -> i32`
- `cpu_count() -> i32`
- `platform() -> String`
- `arch() -> String`
- `target() -> String`
- `board() -> String`
- `is_embedded() -> bool`
- `is_wasm() -> bool`
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
from network import tcp_listen, tcp_bind, udp_bind, tcp_send, udp_send
```

Exports:

- `tcp_connect(host: String, port: i32) -> bool`
- `tcp_connect_timeout(host: String, port: i32, timeout_ms: i32) -> bool`
- `tcp_probe(host: String, port: i32) -> bool`
- `tcp_probe_timeout(host: String, port: i32, timeout_ms: i32) -> bool`
- `tcp_listen(host: String, port: i32) -> bool`
- `tcp_bind(host: String, port: i32) -> bool`
- `udp_bind(host: String, port: i32) -> bool`
- `tcp_send(host: String, port: i32, data: String) -> bool`
- `udp_send(host: String, port: i32, data: String) -> bool`

Current implemented network scope:

- TCP client connectivity probes
- timeout-aware TCP probe
- TCP bind/listen availability checks
- UDP bind availability checks

Not implemented in this module yet:

- TCP server accept/send/receive
- UDP send/receive
- HTTP
- WebSocket

## `fs`

```rune
from fs import exists, read_string, read_text, write_string, write_text
from fs import remove, remove_file, rename, copy
from fs import create_dir, create_dir_all, mkdir, mkdirs
```

Exports:

- `exists(path: String) -> bool`
- `read_string(path: String) -> String`
- `read_text(path: String) -> String`
- `write_string(path: String, content: String) -> bool`
- `write_text(path: String, content: String) -> bool`
- `remove(path: String) -> bool`
- `remove_file(path: String) -> bool`
- `rename(from_path: String, to_path: String) -> bool`
- `copy(from_path: String, to_path: String) -> bool`
- `create_dir(path: String) -> bool`
- `mkdir(path: String) -> bool`
- `create_dir_all(path: String) -> bool`
- `mkdirs(path: String) -> bool`

Current implemented filesystem scope:

- existence checks
- reading UTF-8 text files
- writing UTF-8 text files
- removing files or directories
- renaming files or directories
- copying files
- creating directories recursively or non-recursively

Not implemented in this module yet:

- directory walking
- directory listing

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
For Arduino targets, prefer the shared Rune I/O surface where possible:

- `print(...)`
- `println(...)`
- `input()`

These lower to serial I/O on the Uno target. The `uart_*` functions remain available in `arduino` for lower-level byte-oriented serial control.
