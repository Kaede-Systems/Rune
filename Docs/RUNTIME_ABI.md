# Rune Runtime ABI

## Purpose

This document defines the runtime-facing ABI used by compiled Rune programs.

Rune is a compiled language. Its core execution model is:

- Rune source
- parser / semantics / IR
- backend code generation
- target object or executable output

So the runtime contract for Rune is not an interpreter trait. It is an ABI surface between generated code and a target host runtime.

## Design Rule

The canonical model is:

- compiled Rune code calls runtime symbols
- each target host provides those symbols
- the same logical runtime surface may be satisfied differently on native, WASM, WASI, or other targets

This keeps Rune aligned with its actual compiler architecture.

## Current Runtime Surface

The current runtime surface is expressed through `rune_rt_*` functions.

Implemented areas today include:

- console output
- stderr output
- input
- panic
- time
- system/process helpers
- environment access
- selected networking probes
- dynamic value helpers
- string conversion helpers

## Current Symbol Families

### Output

- `rune_rt_print_i64`
- `rune_rt_eprint_i64`
- `rune_rt_print_str`
- `rune_rt_eprint_str`
- `rune_rt_print_newline`
- `rune_rt_eprint_newline`
- `rune_rt_flush_stdout`
- `rune_rt_flush_stderr`

### Input

- `rune_rt_input_line`
- `rune_rt_last_string_len`

### Panic / Exception

- `rune_rt_panic`
- `rune_rt_fail`
- `rune_rt_raise`

Current runtime error codes:

- `E1001`: division by zero
- `E1002`: modulo by zero
- `E1100`: wall clock unavailable on current target

### String Helpers

- `rune_rt_string_concat`
- `rune_rt_string_from_i64`
- `rune_rt_string_from_bool`
- `rune_rt_string_to_i64`
- `rune_rt_dynamic_to_string`
- `rune_rt_dynamic_to_i64`

### Dynamic Value Helpers

- `rune_rt_print_dynamic`
- `rune_rt_eprint_dynamic`
- `rune_rt_dynamic_binary`
- `rune_rt_dynamic_compare`
- `rune_rt_dynamic_truthy`

### Time

- `rune_rt_time_now_unix`
- `rune_rt_time_monotonic_ms`
- `rune_rt_time_monotonic_us`
- `rune_rt_time_sleep_ms`
- `rune_rt_time_sleep_us`

Notes:

- `rune_rt_time_monotonic_*` and `rune_rt_time_sleep_*` are available on host and embedded targets in the current scope.
- `rune_rt_time_now_unix` is a real wall-clock call on host targets.
- On bare embedded targets without an RTC-backed wall clock, `rune_rt_time_now_unix` fails with `E1100` instead of inventing a fake timestamp.

### System

- `rune_rt_system_pid`
- `rune_rt_system_cpu_count`
- `rune_rt_system_exit`

### Environment

- `rune_rt_env_exists`
- `rune_rt_env_get_i32`
- `rune_rt_env_get_bool`
- `rune_rt_env_arg_count`

### Networking

- `rune_rt_network_tcp_connect`
- `rune_rt_network_tcp_connect_timeout`

## ABI Rules

### Integer Widths

- `i32` uses a 32-bit signed integer ABI
- `i64` uses a 64-bit signed integer ABI
- `bool` is target-lowered but logically boolean

### Strings

Current string ABI is pointer-plus-length.

Logical form:

- `ptr: *const u8`
- `len: usize` or `i64` depending on the generated backend ABI surface

For current LLVM/WASM host glue, the effective runtime contract is:

- string data is UTF-8
- host import/export boundaries use `(ptr, len)`

### Dynamic Values

Current native runtime uses a tagged triple model:

- tag
- payload
- extra

This is an implementation detail of the current runtime slice, not yet a stable public Rune FFI object model.

## Target Model

Each target provides the same logical runtime surface differently.

### Native

Native targets use linked runtime code and target OS facilities.

Examples:

- console I/O
- process ID
- sleep
- environment variables
- host networking

### WASM JS Host

Current `wasm32-unknown-unknown` uses:

- generated `.wasm`
- generated JS host sidecar
- host-provided imports that satisfy `rune_rt_*`

Currently verified in Node for:

- output
- input
- panic
- time
- system
- env
- TCP probe imports

### WASI

WASI is planned as a separate runtime target.

It should satisfy the same logical runtime surface where the host capability model permits it.

### Embedded

Embedded support is deferred.

It should eventually use a smaller target-specific runtime surface rather than pretending all native facilities exist.

## Networking Truth

Rune must not pretend networking capabilities are identical across targets.

### Native

Real host networking can be exposed directly.

### Node-hosted WASM

Networking can be host-backed through JS imports.

### Browser-hosted WASM

Raw TCP and raw UDP are not generally available.

Browser targets should expose host-backed:

- `fetch` / HTTP
- WebSocket
- browser-safe timing / console / storage APIs

They should not claim native raw socket parity.

### WASI

Networking availability depends on the runtime and capability model.

WASI networking must be documented as capability-dependent, not universal.

## Near-Term Runtime ABI Direction

### Stable compiled-host contract

Rune should continue formalizing the `rune_rt_*` surface as the stable compiler-to-host contract.

### No interpreter-first rewrite

Rune should not be re-centered around a runtime trait as if it were a VM or interpreter.

Traits may still be useful inside runtime implementation code, but they are not the canonical architecture of the language.

### Per-target runtime implementations

The right split is:

- native runtime implementation
- JS host runtime implementation
- WASI runtime implementation
- future embedded runtime implementation

Each one satisfies the same logical contract where possible.

## Current Status

Implemented:

- native runtime ABI slice
- JS-hosted WASM ABI slice for current supported features

Planned:

- fuller JS/browser host ABI
- WASI host ABI
- capability-based target documentation

Unresolved:

- long-term stable public FFI object layout for dynamic values
- full exception ABI
- async runtime ABI across native and wasm
