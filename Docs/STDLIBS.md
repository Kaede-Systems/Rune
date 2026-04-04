# Rune Standard Library

This document lists the Rune standard library modules that are implemented today.

These modules are part of Rune's default stdlib registry.

Current loading model:
- `env`, `time`, `sys`, `system`, `io`, `terminal`, `fs`, `json`, `audio`, `network`, `serial`, `gpio`, `pwm`, and `adc` are now registered directly in Rust and loaded from the built-in module registry.
- the remaining stdlib modules are still loaded from [`stdlib/`](/C:/Users/kaededevkentohinode/KUROX/stdlib) while they are migrated.
- this keeps the migration honest: each module is moved only when its real registry-backed path is working end to end.

Current namespace note:

- `from module import name` imports exported names directly
- `import module` supports namespace-qualified access like `module.name(...)`
- namespace-qualified access is the preferred way to use overlapping short names such as `pwm.pin(...)` and `adc.pin(...)`
- import aliases such as `import module as alias` are not implemented yet

## `arduino`

```rune
from arduino import (
    pin_mode, digital_write, digital_read,
    analog_write, analog_read, analog_reference,
    pulse_in, shift_out, shift_in, tone, no_tone,
    servo_attach, servo_detach, servo_write, servo_write_us,
    servo_pulse_for_angle, servo_write_calibrated,
    delay_ms, delay_us, millis, micros,
    read_line,
    mode_input, mode_output, mode_input_pullup, led_builtin,
    high, low,
    bit_order_lsb_first, bit_order_msb_first,
    analog_ref_default, analog_ref_internal, analog_ref_external,
    uart_begin, uart_available, uart_read_byte, uart_peek_byte, uart_write_byte, uart_write,
    uart_flush,
    interrupts_enable, interrupts_disable,
    random_seed, random_i64, random_range,
)
```

Exports:

- `pin_mode(pin: i64, mode: i64) -> unit`
- `digital_write(pin: i64, value: bool) -> unit`
- `digital_read(pin: i64) -> bool`
- `analog_write(pin: i64, value: i64) -> unit`
- `analog_read(pin: i64) -> i64`
- `analog_in(channel: i64) -> i64`
- `clamp_i64(value: i64, minimum: i64, maximum: i64) -> i64`
- `map_range(value: i64, in_min: i64, in_max: i64, out_min: i64, out_max: i64) -> i64`
- `pwm_write(pin: i64, duty: i64) -> unit`
- `pwm_duty_max() -> i64`
- `digital_out(pin: i64, value: bool) -> unit`
- `digital_toggle(pin: i64) -> unit`
- `digital_in(pin: i64) -> bool`
- `digital_in_pullup(pin: i64) -> bool`
- `analog_read_voltage_mv(pin: i64, reference_mv: i64) -> i64`
- `analog_in_voltage_mv(channel: i64, reference_mv: i64) -> i64`
- `analog_read_percent(pin: i64) -> i64`
- `analog_in_percent(channel: i64) -> i64`
- `analog_reference(mode: i64) -> unit`
- `pulse_in(pin: i64, state: bool, timeout_us: i64) -> i64`
- `shift_out(data_pin: i64, clock_pin: i64, bit_order: i64, value: i64) -> unit`
- `shift_in(data_pin: i64, clock_pin: i64, bit_order: i64) -> i64`
- `tone(pin: i64, frequency_hz: i64, duration_ms: i64) -> unit`
- `no_tone(pin: i64) -> unit`
- `servo_attach(pin: i64) -> bool`
- `servo_detach(pin: i64) -> unit`
- `torque_on(pin: i64) -> bool`
- `torque_off(pin: i64) -> unit`
- `servo_write(pin: i64, angle: i64) -> unit`
- `servo_write_us(pin: i64, pulse_us: i64) -> unit`
- `servo_pulse_for_angle(angle: i64, min_us: i64, max_us: i64) -> i64`
- `servo_write_calibrated(pin: i64, angle: i64, min_us: i64, max_us: i64) -> unit`
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
- `default_reference_mv() -> i64`
- `internal_reference_mv() -> i64`
- `uart_begin(baud: i64) -> unit`
- `uart_available() -> i64`
- `uart_read_byte() -> i64`
- `uart_peek_byte() -> i64`
- `uart_write_byte(value: i64) -> unit`
- `uart_write(text: String) -> unit`
- `uart_flush() -> unit`
- `interrupts_enable() -> unit`
- `interrupts_disable() -> unit`
- `random_seed(seed: i64) -> unit`
- `random_i64(max_value: i64) -> i64`
- `random_range(min_value: i64, max_value: i64) -> i64`

Current implemented Arduino scope:

- packaged Uno-target stdlib resolution through `from arduino import ...`
- serial text output with normal Rune `print` and `println`
- serial line input with normal Rune `input()` and `read_line()`
- byte-oriented UART access with `uart_available`, `uart_read_byte`, and `uart_write_byte`
- board constants and pin/timing helpers
- PWM, pulse timing, tone generation, shift register output, and analog reference selection
- shift register input, interrupt enable/disable control, and Arduino random helpers
- Servo control through the packaged Arduino Servo library
- `servo_write(pin, angle)` is the normal positional-servo API using the Arduino Servo library defaults
- `servo_write_us(pin, pulse_us)` is the lower-level pulse interface and is also useful for calibrated servo control when a specific servo's angle mapping differs from the library defaults
- `servo_pulse_for_angle(...)` and `servo_write_calibrated(...)` make that calibration reusable in ordinary Rune code
- Arduino-style `setup()` / `loop()` entrypoints on the Uno target
- top-level/script-style Rune programs also work on the Uno target, so you can often just write normal top-level statements and `while true:` loops without manually defining `setup()` and `loop()`
- Uno builds use LTO plus linker section garbage collection, and packaged Arduino libraries are compiled only when the Rune program actually uses them

Recommended usage:

- use `print`, `println`, and `input()` when you want the same high-level syntax as desktop Rune
- use `uart_*` only when you specifically need byte-level serial control on the board

Current Uno example files using this surface:

- [hello_arduino.rn](/C:/Users/kaededevkentohinode/KUROX/hello_arduino.rn)
- [serial_math_quiz_arduino.rn](/C:/Users/kaededevkentohinode/KUROX/serial_math_quiz_arduino.rn)
- [buzzer_arduino.rn](/C:/Users/kaededevkentohinode/KUROX/buzzer_arduino.rn)
- [buzzer_serial_control_arduino.rn](/C:/Users/kaededevkentohinode/KUROX/buzzer_serial_control_arduino.rn)
- [servo_serial_control_arduino.rn](/C:/Users/kaededevkentohinode/KUROX/servo_serial_control_arduino.rn)
- [servo_angle_control_arduino.rn](/C:/Users/kaededevkentohinode/KUROX/servo_angle_control_arduino.rn)
- [servo_connector_arduino.rn](/C:/Users/kaededevkentohinode/KUROX/servo_connector_arduino.rn)
- [ultrasonic_distance_arduino.rn](/C:/Users/kaededevkentohinode/KUROX/ultrasonic_distance_arduino.rn)
- [avr_oop_string_test.rn](/C:/Users/kaededevkentohinode/KUROX/avr_oop_string_test.rn)

Current Arduino limitations:

- this module is implemented for the current Uno embedded slice, not full Rune parity
- concrete classes/methods and scalar/string dynamic routing work on the current AVR slice
- AVR string-heavy OOP/dynamic operations now use a bounded rotating temporary-string slot pool in the packaged runtime instead of a single shared string buffer, which makes chained method/string expressions behave correctly on Uno without heap allocation
- full dynamic object parity and richer polymorphism are not implemented on AVR yet

## `gpio`

```rune
from gpio import gpio_pin, pwm_pin, analog_pin
from gpio import pin, pwm, analog
from gpio import led_builtin, delay_ms, millis
```

Exports:

- `gpio_pin(pin: i64) -> GpioPin`
- `pin(pin: i64) -> GpioPin`
- `pwm_pin(pin: i64) -> GpioPwm`
- `pwm(pin: i64) -> GpioPwm`
- `analog_pin(pin: i64) -> GpioAnalogIn`
- `analog(pin: i64) -> GpioAnalogIn`
- plus the shared timing/pin helpers re-exported from the current embedded backend

Current implemented GPIO scope:

- Rust-side built-in module surface with the current low-level operations still backed by the `arduino` layer
- common GPIO-style surface on top of the current Arduino Uno backend
- `GpioPin`
  - `.output()`
  - `.input()`
  - `.input_pullup()`
  - `.write(value: bool)`
  - `.high()`
  - `.low()`
  - `.toggle()`
  - `.read() -> bool`
  - `.read_pullup() -> bool`
  - `.blink(times, on_ms, off_ms)`
- `GpioPwm`
  - `.output()`
  - `.write(duty: i64)`
  - `.max_duty() -> i64`
  - `.off()`
- `GpioAnalogIn`
  - `.read() -> i64`
  - `.read_percent() -> i64`
  - `.read_voltage_mv(reference_mv: i64) -> i64`

Current GPIO limitations:

- today this module is backed by the Arduino Uno target only
- it exists to give Rune a common GPIO-facing surface that can later gain Raspberry Pi and ESP32 backends honestly
- Raspberry Pi and ESP32 are not implemented yet, so `gpio` is not a claim that those targets already work

## `pwm`

```rune
from pwm import pwm_pin, write, max_duty
import pwm
```

Exports:

- `PwmPin`
- `pwm_pin(pin: i64) -> PwmPin`
- `pin(pin: i64) -> PwmPin`
- `write(pin: i64, duty: i64) -> unit`
- `max_duty() -> i64`
- `pin_mode(pin: i64, mode: i64) -> unit`
- `mode_output() -> i64`

`PwmPin` methods:

- `.output()`
- `.write(duty: i64)`
- `.max_duty() -> i64`
- `.off()`

Current implemented PWM scope:

- Rust-side built-in module on top of the current shared GPIO runtime hooks
- works on the current native, LLVM, and Arduino Uno paths
- today this is still backed by the current Uno-style PWM behavior under the hood

Current PWM limitations:

- Raspberry Pi and ESP32 PWM backends are not implemented yet
- this module is a shared surface, not a claim of finished non-Uno embedded support

## `adc`

```rune
from adc import adc_pin, read, read_percent, read_voltage_mv, max
import adc
```

Exports:

- `AdcPin`
- `adc_pin(pin: i64) -> AdcPin`
- `pin(pin: i64) -> AdcPin`
- `read(pin: i64) -> i64`
- `read_percent(pin: i64) -> i64`
- `read_voltage_mv(pin: i64, reference_mv: i64) -> i64`
- `max() -> i64`

`AdcPin` methods:

- `.read() -> i64`
- `.read_percent() -> i64`
- `.read_voltage_mv(reference_mv: i64) -> i64`
- `.max() -> i64`

Current implemented ADC scope:

- Rust-side built-in module on top of the current shared GPIO runtime hooks
- works on the current native, LLVM, and Arduino Uno paths
- voltage conversion is derived from raw ADC value and the supplied reference millivolts

Current ADC limitations:

- Raspberry Pi and ESP32 ADC backends are not implemented yet
- this module is a shared surface, not a claim of finished non-Uno embedded support

## `serial`

```rune
from serial import begin, open, is_open, close
from serial import available, read_byte, read_byte_timeout, peek_byte, recv_line, recv_line_timeout, recv_nonempty_timeout
from serial import flush, write, write_byte, write_line, send, send_line
from serial import send_i64, send_bool, send_line_i64, send_line_bool
from serial import SerialPort, serial_port
```

Exports:

- `begin(baud: i64) -> unit`
- `open(port: String, baud: i64) -> bool`
- `is_open() -> bool`
- `close() -> unit`
- `flush() -> unit`
- `available() -> i64`
- `read_byte() -> i64`
- `read_byte_timeout(timeout_ms: i64) -> i64`
- `peek_byte() -> i64`
- `recv_line() -> String`
- `recv_line_timeout(timeout_ms: i64) -> String`
- `recv_nonempty_timeout(timeout_ms: i64) -> String`
- `write(text: String) -> unit`
- `write_byte(value: i64) -> bool`
- `write_line(text: String) -> unit`
- `send(value: dynamic) -> bool`
- `send_i64(value: i64) -> bool`
- `send_bool(value: bool) -> bool`
- `send_line(value: dynamic) -> bool`
- `send_line_i64(value: i64) -> bool`
- `send_line_bool(value: bool) -> bool`
- `SerialPort`
- `serial_port(port: String, baud: i64) -> SerialPort`

Current implemented serial scope:

- Rust-side built-in module surface for shared serial-facing Rune code
- shared serial-facing Rune surface for embedded and host code
- class-style `SerialPort` wrappers using the same `connect`, `is_open`, `close`, `recv_line`, `recv_line_timeout`, `recv_nonempty`, `recv_nonempty_timeout`, `read_byte_timeout`, `peek_byte`, `write_byte`, `send`, and `send_line` method names
- typed serial helpers are also available as `send_i64`, `send_bool`, `send_line_i64`, and `send_line_bool`
- on Arduino Uno:
  - `begin` lowers to `uart_begin`
  - `open` behaves like `begin`
  - `read_byte_timeout` polls the UART for up to the requested number of milliseconds
  - `peek_byte` lowers to UART peek
  - `write_byte` lowers to UART byte writes and reports success
  - `write` and `write_line` lower to UART writes
  - `send` and `send_line` lower to UART writes and report success
  - `recv_line` lowers to the normal embedded input surface
- on non-embedded targets:
  - `open` opens a host serial port such as `COM5`
  - `is_open`, `close`, `flush`, `available`, `read_byte`, `read_byte_timeout`, `peek_byte`, `write_byte`, `send`, `send_line`, `recv_line`, and `recv_line_timeout` talk to the active host serial connection
  - `write` / `write_line` still lower to `print` / `println`

Current serial limitations:

- this is a single-active-connection text serial layer, not a full multi-port device API
- text line input stays on the normal Rune `input()` surface
- lower-level byte control remains in `arduino` via `uart_*`
- current host serial scope is for native host builds, not browser/WASM targets
- `available()` returns `0` and `read_byte()` returns `-1` when no host serial connection is open
- `recv_nonempty_timeout` returns `""` when the timeout expires instead of retrying forever
- `peek_byte()` returns `-1` when no byte is available or no host serial connection is open

## `json`

```rune
from json import parse, stringify, kind, is_null, len, get, index
from json import to_string, to_i64, to_bool
from json import as_string, as_text, as_i64, as_bool, value_kind
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
- `as_string(value: Json) -> String`
- `as_text(value: Json) -> String`
- `as_i64(value: Json) -> i64`
- `as_bool(value: Json) -> bool`
- `value_kind(value: Json) -> String`

Current implemented JSON scope:

- parsing validated JSON text into a first-class `Json` value
- stringifying full JSON values
- object field lookup
- array indexing
- null checks
- container length
- scalar conversion to `String`, `i64`, and `bool`
- convenience aliases for conversion and kind helpers:
  - `as_string`
  - `as_text`
  - `as_i64`
  - `as_bool`
  - `value_kind`

Current JSON limitations:

- direct `Json == Json` and `Json != Json` are implemented as structural comparisons
- JSON values are runtime-backed, not native arrays/maps in the language type system yet

## `io`

```rune
from io import write, writeln, error, errorln, flush_out, flush_err, read_line
from io import stdout_write, stdout_writeln, stderr_write, stderr_writeln
from io import flush_stdout, flush_stderr, prompt, error_prompt
```

Exports:

- `write(value) -> unit`
- `writeln(value) -> unit`
- `error(value) -> unit`
- `errorln(value) -> unit`
- `flush_out() -> unit`
- `flush_err() -> unit`
- `read_line() -> String`
- `stdout_write(value) -> unit`
- `stdout_writeln(value) -> unit`
- `stderr_write(value) -> unit`
- `stderr_writeln(value) -> unit`
- `flush_stdout() -> unit`
- `flush_stderr() -> unit`
- `prompt(message: String) -> String`
- `error_prompt(message: String) -> String`

Current implemented IO scope:

- stdout writes
- stderr writes
- explicit stdout/stderr flush
- line input
- explicit stream-named aliases for callers that prefer more descriptive APIs
- prompt helpers that write, flush, and read in one step

## `time`

```rune
from time import unix_now, monotonic_ms, monotonic_us, sleep_ms, sleep_us, sleep, sleep_until, sleep_until_us
```

Exports:

- `unix_now() -> i64`
- `monotonic_ms() -> i64`
- `monotonic_us() -> i64`
- `sleep_ms(ms: i64) -> unit`
- `sleep_us(us: i64) -> unit`
- `sleep(seconds: i64) -> unit`
- `sleep_until(deadline_ms: i64) -> unit`
- `sleep_until_us(deadline_us: i64) -> unit`

## `system`

```rune
from system import pid, cpu_count, platform, arch, target, board, is_embedded, is_wasm, exit, quit, exit_success, exit_failure

from sys import platform, arch, target, board, is_embedded, is_wasm
from sys import is_host, is_desktop, is_windows, is_linux, is_macos
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
- `is_host() -> bool`
- `is_desktop() -> bool`
- `is_windows() -> bool`
- `is_linux() -> bool`
- `is_macos() -> bool`
- `exit(code: i32) -> unit`
- `quit(code: i32) -> unit`
- `exit_success() -> unit`
- `exit_failure() -> unit`

## `env`

```rune
 from env import has, get, get_i32, get_bool, arg_count, arg
from env import get_or_empty, get_i32_or_zero, get_bool_or_false, get_bool_or_true, arg_or
```

Exports:

- `has(name: String) -> bool`
- `get(name: String, default: String) -> String`
- `get_i32(name: String, default: i32) -> i32`
- `get_bool(name: String, default: bool) -> bool`
- `arg_count() -> i32`
- `arg(index: i32) -> String`
- `arg_or(index: i32, default: String) -> String`
- `get_or_empty(name: String) -> String`
- `get_i32_or_zero(name: String) -> i32`
- `get_bool_or_false(name: String) -> bool`
- `get_bool_or_true(name: String) -> bool`

Notes:

- `arg_count()` and `arg(index)` use only user arguments, not the executable path
- out-of-range indexes return `""`
- missing environment variables return the provided default for `get(...)`
- `arg_or(...)` provides an explicit fallback string when the requested argument is missing

## `network`

```rune
from network import tcp_connect, tcp_connect_timeout
from network import tcp_probe, tcp_probe_timeout
from network import tcp_listen, tcp_bind, udp_bind, tcp_send, udp_send
from network import tcp_send_line, udp_send_line, tcp_recv, tcp_recv_timeout, udp_recv
from network import tcp_request, request, request_line, recv, recv_timeout, recv_udp
from network import tcp_accept_once, accept_once, tcp_reply_once, reply_once, reply_once_line
from network import tcp_server_open, tcp_server_accept, tcp_server_reply, tcp_server_reply_line, tcp_server_close
from network import last_error_code, last_error, clear_error
from network import connect, connect_timeout, probe, probe_timeout
from network import listen, bind, send, send_line, send_udp, send_line_udp
from network import TcpClient, TcpServer, UdpEndpoint, tcp_client, tcp_server, udp_endpoint
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
- `tcp_send_line(host: String, port: i32, value: dynamic) -> bool`
- `udp_send_line(host: String, port: i32, value: dynamic) -> bool`
- `tcp_recv(host: String, port: i32, max_bytes: i32) -> String`
- `tcp_recv_timeout(host: String, port: i32, max_bytes: i32, timeout_ms: i32) -> String`
- `udp_recv(host: String, port: i32, max_bytes: i32, timeout_ms: i32) -> String`
- `tcp_request(host: String, port: i32, data: String, max_bytes: i32, timeout_ms: i32) -> String`
- `request(host: String, port: i32, data: String, max_bytes: i32, timeout_ms: i32) -> String`
- `request_line(host: String, port: i32, value: dynamic, max_bytes: i32, timeout_ms: i32) -> String`
- `tcp_accept_once(host: String, port: i32, max_bytes: i32, timeout_ms: i32) -> String`
- `accept_once(host: String, port: i32, max_bytes: i32, timeout_ms: i32) -> String`
- `tcp_reply_once(host: String, port: i32, data: String, max_bytes: i32, timeout_ms: i32) -> String`
- `reply_once(host: String, port: i32, data: String, max_bytes: i32, timeout_ms: i32) -> String`
- `reply_once_line(host: String, port: i32, value: dynamic, max_bytes: i32, timeout_ms: i32) -> String`
- `tcp_server_open(host: String, port: i32) -> i32`
- `tcp_client_open(host: String, port: i32, timeout_ms: i32) -> i32`
- `tcp_server_accept(handle: i32, max_bytes: i32, timeout_ms: i32) -> String`
- `tcp_client_recv(handle: i32, max_bytes: i32, timeout_ms: i32) -> String`
- `tcp_server_reply(handle: i32, data: String, max_bytes: i32, timeout_ms: i32) -> String`
- `tcp_client_send(handle: i32, data: String) -> bool`
- `tcp_client_send_line(handle: i32, value: dynamic) -> bool`
- `tcp_server_reply_line(handle: i32, value: dynamic, max_bytes: i32, timeout_ms: i32) -> String`
- `tcp_client_close(handle: i32) -> bool`
- `tcp_server_close(handle: i32) -> bool`
- `last_error_code() -> i32`
- `last_error() -> String`
- `clear_error() -> unit`
- `connect(host: String, port: i32) -> bool`
- `connect_timeout(host: String, port: i32, timeout_ms: i32) -> bool`
- `probe(host: String, port: i32) -> bool`
- `probe_timeout(host: String, port: i32, timeout_ms: i32) -> bool`
- `listen(host: String, port: i32) -> bool`
- `bind(host: String, port: i32) -> bool`
- `send(host: String, port: i32, data: String) -> bool`
- `send_line(host: String, port: i32, value: dynamic) -> bool`
- `send_udp(host: String, port: i32, data: String) -> bool`
- `send_line_udp(host: String, port: i32, value: dynamic) -> bool`
- `TcpClient`
- `TcpServer`
- `UdpEndpoint`
- `tcp_client(host: String, port: i32) -> TcpClient`
- `tcp_server(host: String, port: i32) -> TcpServer`
- `udp_endpoint(host: String, port: i32) -> UdpEndpoint`

Current implemented network scope:

- TCP client connectivity probes
- timeout-aware TCP probe
- TCP bind/listen availability checks
- UDP bind availability checks
- TCP/UDP send convenience wrappers for dynamic values and lines
- TCP receive helpers returning `String`
- timeout-aware TCP receive
- TCP request/response helpers returning `String`
- TCP one-shot server accept helpers returning the received payload as `String`
- TCP one-shot reply helpers that return the received request body as `String`
- low-level persistent TCP server handles through `tcp_server_open`, `tcp_server_accept`, `tcp_server_reply`, and `tcp_server_close`
- low-level persistent TCP client handles through `tcp_client_open`, `tcp_client_send`, `tcp_client_send_line`, `tcp_client_recv`, and `tcp_client_close`
- UDP receive helpers returning `String`
- last network error inspection through `last_error_code()` and `last_error()`
- explicit error-state reset through `clear_error()`
- class-style client/endpoint wrappers using the same `connect`, `probe`, `send`, and `send_line` names
- class-style client/endpoint wrappers using the same `connect`, `bind`, `probe`, `send`, `send_line`, `recv`, `recv_timeout`, `request`, `request_line`, `send_text`, and `send_line_text` names
- `TcpServer` exposes `listen`, `bind`, `accept_once`, `reply_once`, `reply_once_line`, and `reply_once_text`
- `TcpServer` also exposes higher-level persistent-handle helpers:
  - `open_handle()`
  - `accept(handle, max_bytes, timeout_ms)`
  - `reply(handle, value, max_bytes, timeout_ms)`
  - `reply_line(handle, value, max_bytes, timeout_ms)`
  - `reply_text(handle, value, max_bytes, timeout_ms)`
  - `close_handle(handle)`
- `TcpClient` also exposes higher-level persistent-handle helpers:
  - `open_handle(timeout_ms)`
  - `send_handle(handle, value)`
  - `send_line_handle(handle, value)`
  - `recv_handle(handle, max_bytes, timeout_ms)`
  - `close_handle(handle)`
- class-style client/endpoint wrappers also expose `send_text` and `send_line_text` for callers that prefer explicit text payloads
- the current receive/request slice is verified on native and LLVM executable paths
- the current one-shot server accept/reply slice is also verified on native and LLVM executable paths
- the current low-level persistent TCP server-handle slice is also verified on native and LLVM executable paths
- the current low-level and class-style persistent TCP client-handle slices are also verified on native and LLVM executable paths
- the current error-state slice is verified on native and LLVM executable paths
- Uno-class Arduino targets do not claim direct `network` support without a real backing network stack

Current network error codes:

- `0`: no error
- `1`: invalid argument
- `2`: unsupported target
- `3`: address resolution failed
- `4`: bind/listen failed
- `5`: connect failed
- `6`: accept timed out
- `7`: accept failed
- `8`: read failed
- `9`: write failed
- `10`: socket option setup failed

Not implemented in this module yet:

- persistent TCP server socket lifecycle and multi-client accept loops
- close/shutdown-style explicit socket lifecycle control
- HTTP
- WebSocket

## `fs`

```rune
from fs import exists, read, read_string, read_text, write, write_string, write_text
from fs import remove, delete, remove_file, rename, move, copy
from fs import create_dir, create_dir_all, mkdir, mkdir_p, mkdirs
```

Exports:

- `exists(path: String) -> bool`
- `read(path: String) -> String`
- `read_string(path: String) -> String`
- `read_text(path: String) -> String`
- `write(path: String, content: String) -> bool`
- `write_string(path: String, content: String) -> bool`
- `write_text(path: String, content: String) -> bool`
- `remove(path: String) -> bool`
- `delete(path: String) -> bool`
- `remove_file(path: String) -> bool`
- `rename(from_path: String, to_path: String) -> bool`
- `move(from_path: String, to_path: String) -> bool`
- `copy(from_path: String, to_path: String) -> bool`
- `create_dir(path: String) -> bool`
- `mkdir(path: String) -> bool`
- `create_dir_all(path: String) -> bool`
- `mkdir_p(path: String) -> bool`
- `mkdirs(path: String) -> bool`

Current implemented filesystem scope:

- existence checks
- reading UTF-8 text files
- writing UTF-8 text files
- removing files or directories
- renaming files or directories
- copying files
- creating directories recursively or non-recursively
- convenience aliases for common filesystem verbs:
  - `read`
  - `write`
  - `delete`
  - `move`
  - `mkdir_p`

Not implemented in this module yet:

- directory walking
- directory listing

## `terminal`

```rune
from terminal import clear, move_to, hide_cursor, show_cursor, set_title
from terminal import clear_screen, home, clear_and_home, hide, show
from terminal import cursor_hide, cursor_show, move_cursor, title
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
- `cursor_hide() -> unit`
- `cursor_show() -> unit`
- `move_cursor(row: i32, col: i32) -> unit`
- `title(text: String) -> unit`

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

High-level hardware classes:
- `DigitalPin(pin=...)`
  - `.output()`
  - `.input()`
  - `.input_pullup()`
  - `.write(value: bool)`
  - `.high()`
  - `.low()`
  - `.read() -> bool`
  - `.toggle()`
  - `.blink(times, on_ms, off_ms)`
  - `.pulse(on_ms, off_ms)`
- `PwmPin(pin=...)`
  - `.output()`
  - `.write(duty: i64)`
  - `.max_duty() -> i64`
  - `.off()`
- `AnalogPin(pin=...)`
  - `.read() -> i64`
  - `.read_voltage_mv(reference_mv: i64) -> i64`
  - `.read_percent() -> i64`

Voltage note:
- Arduino Uno does not have a true DAC, so Rune cannot set an arbitrary analog voltage directly on normal Uno pins.
- `analog_write` / `pwm_write` / `PwmPin.write(...)` use PWM duty-cycle output, not real steady analog voltage.
- `analog_read_voltage_mv` converts ADC readings into approximate millivolts using the supplied reference voltage.
- `pwm_write(pin, 128)` on the Uno means about a 50% duty cycle on the normal 8-bit PWM scale.
- Because PWM is switching quickly, the average power can behave somewhat like a lower analog level for LEDs, motors, and filtered circuits, but it is still digital switching, not a true fixed voltage output.

Function-first pin usage:
- use plain pin numbers directly, for example `digital_out(7, true)` or `pwm_write(9, 128)`
- `digital_out`, `digital_in`, and `digital_in_pullup` provide a simpler direct style on top of `pin_mode` + `digital_*`
- use `analog_in(0)` / `analog_in_voltage_mv(0, 5000)` when you want Uno analog channels in the simple `A0..A5` style without extra wrappers

Example device-style programs in the repo:
- [buzzer_arduino.rn](/C:/Users/kaededevkentohinode/KUROX/buzzer_arduino.rn)
- [buzzer_serial_control_arduino.rn](/C:/Users/kaededevkentohinode/KUROX/buzzer_serial_control_arduino.rn)
- [serial_math_quiz_arduino.rn](/C:/Users/kaededevkentohinode/KUROX/serial_math_quiz_arduino.rn)
- [ultrasonic_distance_arduino.rn](/C:/Users/kaededevkentohinode/KUROX/ultrasonic_distance_arduino.rn)
