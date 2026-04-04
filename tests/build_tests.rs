use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use rune::build::{
    build_executable, build_executable_llvm, build_executable_llvm_with_options, build_object_file,
    build_shared_library, build_static_library, default_library_extension, supported_targets,
    target_spec, BuildError, BuildOptions,
};

fn temp_dir() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should be after epoch")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("rune-build-test-{stamp}"));
    fs::create_dir_all(&dir).expect("failed to create temp dir");
    dir
}

fn build_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn assert_no_zig_linking_gap(result: Result<(), BuildError>) {
    match result {
        Err(BuildError::ToolNotFound(message)) => {
            assert!(
                message.contains("Zig is no longer used")
                    || message.contains("require packaged")
                    || message.contains("requires packaged"),
                "unexpected tool gap message: {message}"
            );
        }
        other => panic!("expected explicit packaged-toolchain gap, got {other:?}"),
    }
}

#[test]
fn chooses_host_library_extension() {
    let ext = default_library_extension();
    assert!(matches!(ext, "dll" | "so" | "dylib"));
}

#[test]
fn exposes_known_cross_targets() {
    let targets = supported_targets();
    assert!(targets
        .iter()
        .any(|target| target.triple == "x86_64-unknown-linux-gnu"));
    assert!(targets
        .iter()
        .any(|target| target.triple == "x86_64-apple-darwin"));
    assert!(targets
        .iter()
        .any(|target| target.triple == "x86_64-pc-windows-gnu"));
    assert!(targets
        .iter()
        .any(|target| target.triple == "aarch64-pc-windows-gnu"));
    assert!(targets
        .iter()
        .any(|target| target.triple == "wasm32-unknown-unknown"));
    assert!(targets
        .iter()
        .any(|target| target.triple == "thumbv6m-none-eabi"));
    assert!(targets
        .iter()
        .any(|target| target.triple == "riscv32-unknown-elf"));
    assert!(targets
        .iter()
        .any(|target| target.triple == "avr-atmega328p-arduino-uno"));
}

#[test]
fn resolves_target_specific_extensions() {
    let linux = target_spec(Some("x86_64-unknown-linux-gnu")).expect("linux target should resolve");
    assert_eq!(linux.exe_extension, "");
    assert_eq!(linux.library_extension, "so");
    assert_eq!(linux.static_library_extension, "a");

    let mac = target_spec(Some("x86_64-apple-darwin")).expect("mac target should resolve");
    assert_eq!(mac.library_extension, "dylib");
    assert_eq!(mac.static_library_extension, "a");

    let windows =
        target_spec(Some("x86_64-pc-windows-gnu")).expect("windows target should resolve");
    assert_eq!(windows.exe_extension, "exe");
    assert_eq!(windows.library_extension, "dll");
    assert_eq!(windows.static_library_extension, "lib");

    let wasm = target_spec(Some("wasm32-unknown-unknown")).expect("wasm target should resolve");
    assert_eq!(wasm.exe_extension, "wasm");
    assert_eq!(wasm.library_extension, "wasm");
    assert_eq!(wasm.static_library_extension, "a");
    assert_eq!(wasm.object_extension, "o");

    let embedded = target_spec(Some("thumbv6m-none-eabi")).expect("embedded target should resolve");
    assert_eq!(embedded.exe_extension, "");
    assert_eq!(embedded.library_extension, "a");
    assert_eq!(embedded.static_library_extension, "a");
    assert_eq!(embedded.object_extension, "o");

    let uno =
        target_spec(Some("avr-atmega328p-arduino-uno")).expect("arduino uno target should resolve");
    assert_eq!(uno.exe_extension, "hex");
    assert_eq!(uno.library_extension, "a");
    assert_eq!(uno.static_library_extension, "a");
    assert_eq!(uno.object_extension, "o");
}

#[test]
fn resolves_host_default_target_sensibly() {
    let host = target_spec(None).expect("host target should resolve");

    if cfg!(target_os = "windows") {
        assert_eq!(host.triple, "x86_64-pc-windows-gnu");
    } else if cfg!(target_os = "macos") {
        assert_eq!(host.triple, "x86_64-apple-darwin");
    } else {
        assert_eq!(host.triple, "x86_64-unknown-linux-gnu");
    }
}

#[test]
fn reports_unsupported_target_backend_clearly() {
    let error = BuildError::UnsupportedBackendForTarget("unsupported-target".to_string());
    assert!(error
        .to_string()
        .contains("requires a target-aware backend"));
}

#[test]
fn builds_linux_elf_via_llvm_backend() {
    let _guard = build_lock()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let dir = temp_dir();
    let source_path = dir.join("linux_main.rn");
    let output_path = dir.join("linux_main");

    fs::write(&source_path, "def main() -> i32:\n    return 42\n").expect("failed to write source");

    assert_no_zig_linking_gap(build_executable(
        &source_path,
        &output_path,
        Some("x86_64-unknown-linux-gnu"),
    ));
}

#[test]
fn builds_linux_arm64_elf_via_llvm_backend() {
    let _guard = build_lock()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let dir = temp_dir();
    let source_path = dir.join("linux_arm64_main.rn");
    let output_path = dir.join("linux_arm64_main");

    fs::write(&source_path, "def main() -> i32:\n    return 42\n").expect("failed to write source");

    assert_no_zig_linking_gap(build_executable(
        &source_path,
        &output_path,
        Some("aarch64-unknown-linux-gnu"),
    ));
}

#[test]
fn builds_macos_macho_via_llvm_backend() {
    let _guard = build_lock()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let dir = temp_dir();
    let source_path = dir.join("mac_main.rn");
    let output_path = dir.join("mac_main");

    fs::write(&source_path, "def main() -> i32:\n    return 42\n").expect("failed to write source");

    assert_no_zig_linking_gap(build_executable(
        &source_path,
        &output_path,
        Some("x86_64-apple-darwin"),
    ));
}

#[test]
fn builds_macos_arm64_macho_via_llvm_backend() {
    let _guard = build_lock()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let dir = temp_dir();
    let source_path = dir.join("mac_arm64_main.rn");
    let output_path = dir.join("mac_arm64_main");

    fs::write(&source_path, "def main() -> i32:\n    return 42\n").expect("failed to write source");

    assert_no_zig_linking_gap(build_executable(
        &source_path,
        &output_path,
        Some("aarch64-apple-darwin"),
    ));
}

#[test]
fn builds_windows_exe_via_explicit_llvm_backend() {
    let _guard = build_lock()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let dir = temp_dir();
    let source_path = dir.join("windows_main.rn");
    let output_path = dir.join("windows_main.exe");

    fs::write(
        &source_path,
        "def main() -> i32:\n    println(42)\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable_llvm(&source_path, &output_path, Some("x86_64-pc-windows-gnu"))
        .expect("windows llvm build should succeed");

    assert!(output_path.is_file());
}

#[test]
fn builds_windows_exe_via_default_build_backend() {
    let _guard = build_lock()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let dir = temp_dir();
    let source_path = dir.join("windows_default_main.rn");
    let output_path = dir.join("windows_default_main.exe");

    fs::write(&source_path, "def main() -> i32:\n    return 0\n").expect("failed to write source");

    build_executable(&source_path, &output_path, Some("x86_64-pc-windows-gnu"))
        .expect("default build should use llvm backend successfully on windows");

    let bytes = fs::read(&output_path).expect("failed to read windows binary");
    assert!(bytes.starts_with(b"MZ"));
}

#[test]
fn builds_windows_arm64_exe_via_llvm_backend() {
    let _guard = build_lock()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let dir = temp_dir();
    let source_path = dir.join("windows_arm64_main.rn");
    let output_path = dir.join("windows_arm64_main.exe");

    fs::write(&source_path, "def main() -> i32:\n    return 42\n").expect("failed to write source");

    assert_no_zig_linking_gap(build_executable(
        &source_path,
        &output_path,
        Some("aarch64-pc-windows-gnu"),
    ));
}

#[test]
fn builds_wasm_module_via_llvm_backend() {
    let _guard = build_lock()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let dir = temp_dir();
    let source_path = dir.join("module_main.rn");
    let output_path = dir.join("module_main.wasm");

    fs::write(&source_path, "def main() -> i32:\n    return 42\n").expect("failed to write source");

    build_executable(&source_path, &output_path, Some("wasm32-unknown-unknown"))
        .expect("wasm module build should succeed");

    let bytes = fs::read(&output_path).expect("failed to read wasm module");
    assert!(bytes.starts_with(b"\0asm"));
}

#[test]
fn builds_linux_shared_library_via_llvm_backend() {
    let _guard = build_lock()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let dir = temp_dir();
    let source_path = dir.join("linux_lib.rn");
    let output_path = dir.join("linux_lib.so");

    fs::write(
        &source_path,
        "def add(a: i32, b: i32) -> i32:\n    return a + b\n",
    )
    .expect("failed to write source");

    assert_no_zig_linking_gap(build_shared_library(
        &source_path,
        &output_path,
        Some("x86_64-unknown-linux-gnu"),
    ));
}

#[test]
fn builds_linux_arm64_shared_library_via_llvm_backend() {
    let _guard = build_lock()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let dir = temp_dir();
    let source_path = dir.join("linux_arm64_lib.rn");
    let output_path = dir.join("linux_arm64_lib.so");

    fs::write(
        &source_path,
        "def add(a: i32, b: i32) -> i32:\n    return a + b\n",
    )
    .expect("failed to write source");

    assert_no_zig_linking_gap(build_shared_library(
        &source_path,
        &output_path,
        Some("aarch64-unknown-linux-gnu"),
    ));
}

#[test]
fn builds_macos_shared_library_via_llvm_backend() {
    let _guard = build_lock()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let dir = temp_dir();
    let source_path = dir.join("mac_lib.rn");
    let output_path = dir.join("mac_lib.dylib");

    fs::write(
        &source_path,
        "def add(a: i32, b: i32) -> i32:\n    return a + b\n",
    )
    .expect("failed to write source");

    assert_no_zig_linking_gap(build_shared_library(
        &source_path,
        &output_path,
        Some("x86_64-apple-darwin"),
    ));
}

#[test]
fn builds_macos_arm64_shared_library_via_llvm_backend() {
    let _guard = build_lock()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let dir = temp_dir();
    let source_path = dir.join("mac_arm64_lib.rn");
    let output_path = dir.join("mac_arm64_lib.dylib");

    fs::write(
        &source_path,
        "def add(a: i32, b: i32) -> i32:\n    return a + b\n",
    )
    .expect("failed to write source");

    assert_no_zig_linking_gap(build_shared_library(
        &source_path,
        &output_path,
        Some("aarch64-apple-darwin"),
    ));
}

#[test]
fn builds_linux_static_library_via_packaged_llvm_archiver() {
    let _guard = build_lock()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let dir = temp_dir();
    let source_path = dir.join("linux_static_lib.rn");
    let output_path = dir.join("linux_static_lib.a");

    fs::write(
        &source_path,
        "def add(a: i32, b: i32) -> i32:\n    return a + b\n",
    )
    .expect("failed to write source");

    build_static_library(&source_path, &output_path, Some("x86_64-unknown-linux-gnu"))
        .expect("linux static library build should succeed");

    let bytes = fs::read(&output_path).expect("failed to read linux static library");
    assert!(bytes.starts_with(b"!<arch>\n"));
}

#[test]
fn builds_windows_static_library_via_packaged_llvm_archiver() {
    let _guard = build_lock()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let dir = temp_dir();
    let source_path = dir.join("windows_static_lib.rn");
    let output_path = dir.join("windows_static_lib.lib");

    fs::write(
        &source_path,
        "def add(a: i32, b: i32) -> i32:\n    return a + b\n",
    )
    .expect("failed to write source");

    build_static_library(&source_path, &output_path, Some("x86_64-pc-windows-gnu"))
        .expect("windows static library build should succeed");

    let bytes = fs::read(&output_path).expect("failed to read windows static library");
    assert!(bytes.starts_with(b"!<arch>\n"));
    let header = fs::read_to_string(output_path.with_extension("h"))
        .expect("failed to read generated windows static library header");
    assert!(header.contains("int32_t add(int32_t a, int32_t b);"));
}

#[test]
fn builds_thumb_embedded_object_via_packaged_llvm_backend() {
    let dir = temp_dir();
    let source_path = dir.join("thumb_embedded.rn");
    let output_path = dir.join("thumb_embedded.o");

    fs::write(
        &source_path,
        "def add(a: i32, b: i32) -> i32:\n    return a + b\n",
    )
    .expect("failed to write source");

    build_object_file(&source_path, &output_path, Some("thumbv6m-none-eabi"))
        .expect("thumb embedded object build should succeed");

    let bytes = fs::read(&output_path).expect("failed to read thumb embedded object");
    assert!(!bytes.is_empty());
}

#[test]
fn builds_avr_embedded_object_via_avr_capable_llvm_backend_when_available() {
    if rune::toolchain::find_packaged_llvm_avr_tool("llc").is_none() {
        return;
    }

    let dir = temp_dir();
    let source_path = dir.join("avr_embedded.rn");
    let output_path = dir.join("avr_embedded.o");

    fs::write(
        &source_path,
        "def main() -> i32:\n    println(\"avr llvm object\")\n    return 0\n",
    )
    .expect("failed to write source");

    build_object_file(&source_path, &output_path, Some("avr-atmega328p-arduino-uno"))
        .expect("avr embedded object build should succeed");

    let bytes = fs::read(&output_path).expect("failed to read avr embedded object");
    assert!(!bytes.is_empty());
    assert!(bytes.starts_with(&[0x7F, b'E', b'L', b'F']));
}

#[test]
fn builds_riscv32_embedded_static_library_via_packaged_llvm_archiver() {
    let dir = temp_dir();
    let source_path = dir.join("riscv_embedded.rn");
    let output_path = dir.join("riscv_embedded.a");

    fs::write(
        &source_path,
        "def add(a: i32, b: i32) -> i32:\n    return a + b\n",
    )
    .expect("failed to write source");

    build_static_library(&source_path, &output_path, Some("riscv32-unknown-elf"))
        .expect("riscv embedded static library build should succeed");

    let bytes = fs::read(&output_path).expect("failed to read riscv embedded static library");
    assert!(bytes.starts_with(b"!<arch>\n"));
}

#[test]
fn builds_arduino_uno_hex_via_packaged_avr_gcc() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_hello.rn");
    let output_path = dir.join("arduino_uno_hello.hex");

    fs::write(
        &source_path,
        "def main() -> i32:\n    println(\"Hello from Rune\")\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno hex build should succeed");

    let bytes = fs::read(&output_path).expect("failed to read arduino uno hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_hex_with_locals_and_control_flow() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_logic.rn");
    let output_path = dir.join("arduino_uno_logic.hex");

    fs::write(
        &source_path,
        "def main() -> i32:\n    let value = 20 + 22\n    let ok = value == 42\n    if ok:\n        println(value)\n    let counter = 0\n    while counter < 2:\n        println(counter)\n        counter = counter + 1\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno logic hex build should succeed");

    let bytes = fs::read(&output_path).expect("failed to read arduino uno logic hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_hex_with_arduino_stdlib_runtime_calls() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_runtime.rn");
    let output_path = dir.join("arduino_uno_runtime.hex");
    let stdlib_source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("stdlib")
        .join("arduino.rn");
    fs::copy(&stdlib_source, dir.join("arduino.rn")).expect("failed to stage arduino stdlib");

    fs::write(
        &source_path,
        "from arduino import pin_mode, digital_write, digital_read, analog_read, delay_ms, millis\n\n\
         def main() -> i32:\n    pin_mode(13, 1)\n    digital_write(13, true)\n    let started = millis()\n    let level = digital_read(13)\n    let analog = analog_read(0)\n    if level:\n        println(analog)\n    delay_ms(1)\n    println(millis() >= started)\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno stdlib runtime hex build should succeed");

    let bytes = fs::read(&output_path).expect("failed to read arduino uno runtime hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_hex_with_pwm_and_board_constants() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_pwm.rn");
    let output_path = dir.join("arduino_uno_pwm.hex");
    let stdlib_source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("stdlib")
        .join("arduino.rn");
    fs::copy(&stdlib_source, dir.join("arduino.rn")).expect("failed to stage arduino stdlib");

    fs::write(
        &source_path,
        "from arduino import pin_mode, analog_write, delay_us, micros, mode_output, led_builtin\n\n\
         def main() -> i32:\n    let led: i64 = led_builtin()\n    let output_mode: i64 = mode_output()\n    pin_mode(led, output_mode)\n    analog_write(led, 128)\n    delay_us(10)\n    println(micros() >= 0)\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno pwm hex build should succeed");

    let bytes = fs::read(&output_path).expect("failed to read arduino uno pwm hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_hex_with_uart_calls() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_uart.rn");
    let output_path = dir.join("arduino_uno_uart.hex");
    let stdlib_source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("stdlib")
        .join("arduino.rn");
    fs::copy(&stdlib_source, dir.join("arduino.rn")).expect("failed to stage arduino stdlib");

    fs::write(
        &source_path,
        "from arduino import uart_begin, uart_available, uart_read_byte, uart_write, uart_write_byte\n\n\
         def main() -> i32:\n    uart_begin(115200)\n    uart_write(\"Rune UART ready\")\n    uart_write_byte(10)\n    let available: i64 = uart_available()\n    if available > 0:\n        let value: i64 = uart_read_byte()\n        println(value)\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno uart hex build should succeed");

    let bytes = fs::read(&output_path).expect("failed to read arduino uno uart hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_serial_calculator_example() {
    let dir = temp_dir();
    let source_path = dir.join("serial_calculator_arduino.rn");
    let output_path = dir.join("serial_calculator_arduino.hex");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    fs::copy(
        root.join("stdlib").join("arduino.rn"),
        dir.join("arduino.rn"),
    )
    .expect("failed to stage arduino stdlib");
    fs::copy(root.join("serial_calculator_arduino.rn"), &source_path)
        .expect("failed to stage serial calculator example");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno serial calculator example should build");

    let bytes = fs::read(&output_path).expect("failed to read serial calculator hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_serial_math_quiz_example() {
    let dir = temp_dir();
    let source_path = dir.join("serial_math_quiz_arduino.rn");
    let output_path = dir.join("serial_math_quiz_arduino.hex");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    fs::copy(root.join("serial_math_quiz_arduino.rn"), &source_path)
        .expect("failed to stage serial math quiz example");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno serial math quiz example should build");

    let bytes = fs::read(&output_path).expect("failed to read serial math quiz hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_with_serial_class_wrapper() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_serial_class.rn");
    let output_path = dir.join("arduino_uno_serial_class.hex");

    fs::write(
        &source_path,
        "from serial import serial_port\n\n\
         def setup() -> unit:\n    let serial = serial_port(\"COM5\", 115200)\n    if serial.connect():\n        serial.send_line(\"hello\")\n    return\n\n\
         def loop() -> unit:\n    return\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno serial class wrapper example should build");

    let bytes = fs::read(&output_path).expect("failed to read serial class hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_host_serial_connector_example() {
    let dir = temp_dir();
    let source_path = dir.join("serial_connector_arduino.rn");
    let output_path = dir.join(if cfg!(windows) {
        "serial_connector_arduino.exe"
    } else {
        "serial_connector_arduino"
    });
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    fs::copy(root.join("serial_connector_arduino.rn"), &source_path)
        .expect("failed to stage serial connector example");

    build_executable(&source_path, &output_path, None)
        .expect("host serial connector example should build");

    assert!(
        output_path.is_file(),
        "expected host connector output to exist"
    );
}

#[test]
fn builds_host_serial_typed_send_helpers_example() {
    let dir = temp_dir();
    let source_path = dir.join("serial_typed_send_helpers.rn");
    let output_path = dir.join(if cfg!(windows) {
        "serial_typed_send_helpers.exe"
    } else {
        "serial_typed_send_helpers"
    });

    fs::write(
        &source_path,
        "from serial import send_bool, send_i64, send_line_bool, send_line_i64\n\n\
         def main() -> i32:\n    println(send_i64(42))\n    println(send_bool(true))\n    println(send_line_i64(7))\n    println(send_line_bool(false))\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &output_path, None)
        .expect("host serial typed send helpers example should build");

    assert!(
        output_path.is_file(),
        "expected typed send helpers output to exist"
    );
}

#[test]
fn builds_host_serial_reader_example() {
    let dir = temp_dir();
    let source_path = dir.join("serial_reader.rn");
    let output_path = dir.join(if cfg!(windows) {
        "serial_reader.exe"
    } else {
        "serial_reader"
    });
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    fs::copy(root.join("serial_reader.rn"), &source_path)
        .expect("failed to stage serial reader example");

    build_executable(&source_path, &output_path, None)
        .expect("host serial reader example should build");

    assert!(
        output_path.is_file(),
        "expected host serial reader output to exist"
    );
}

#[test]
fn builds_host_servo_connector_example() {
    let dir = temp_dir();
    let source_path = dir.join("servo_connector_arduino.rn");
    let output_path = dir.join(if cfg!(windows) {
        "servo_connector_arduino.exe"
    } else {
        "servo_connector_arduino"
    });
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    fs::copy(root.join("servo_connector_arduino.rn"), &source_path)
        .expect("failed to stage servo connector example");

    build_executable(&source_path, &output_path, None)
        .expect("host servo connector example should build");

    assert!(
        output_path.is_file(),
        "expected host servo connector output to exist"
    );
}

#[test]
fn builds_arduino_uno_with_shared_input_print_surface() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_shared_io.rn");
    let output_path = dir.join("arduino_uno_shared_io.hex");
    let stdlib_source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("stdlib")
        .join("arduino.rn");
    fs::copy(&stdlib_source, dir.join("arduino.rn")).expect("failed to stage arduino stdlib");

    fs::write(
        &source_path,
        "from arduino import uart_begin\n\n\
         def main() -> i32:\n    uart_begin(115200)\n    print(\"name> \")\n    let name: String = input()\n    println(name)\n    let value: i64 = int(\"123\")\n    println(value)\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno shared input/print build should succeed");

    let bytes = fs::read(&output_path).expect("failed to read arduino uno shared io hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_with_user_defined_helper_functions() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_helpers.rn");
    let output_path = dir.join("arduino_uno_helpers.hex");
    let stdlib_source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("stdlib")
        .join("arduino.rn");
    fs::copy(&stdlib_source, dir.join("arduino.rn")).expect("failed to stage arduino stdlib");

    fs::write(
        &source_path,
        "from arduino import uart_begin\n\n\
         def format_sum(left: i64, right: i64) -> String:\n    return str(left + right)\n\n\
         def write_banner() -> unit:\n    println(\"Rune helpers ready\")\n\n\
         def main() -> i32:\n    uart_begin(115200)\n    write_banner()\n    println(format_sum(20, 22))\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno helper function build should succeed");

    let bytes = fs::read(&output_path).expect("failed to read arduino uno helper hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_with_setup_and_loop_entrypoints() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_setup_loop.rn");
    let output_path = dir.join("arduino_uno_setup_loop.hex");
    let stdlib_source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("stdlib")
        .join("arduino.rn");
    fs::copy(&stdlib_source, dir.join("arduino.rn")).expect("failed to stage arduino stdlib");

    fs::write(
        &source_path,
        "from arduino import delay_ms, digital_write, led_builtin, mode_output, pin_mode\n\n\
         def setup() -> unit:\n    let led: i64 = led_builtin()\n    pin_mode(led, mode_output())\n\n\
         def loop() -> unit:\n    let led: i64 = led_builtin()\n    digital_write(led, true)\n    delay_ms(5)\n    digital_write(led, false)\n    delay_ms(5)\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno setup/loop build should succeed");

    let bytes = fs::read(&output_path).expect("failed to read arduino uno setup/loop hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_stdlib_wrappers_without_recursive_calls() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_wrapper_lowering.rn");
    let output_path = dir.join("arduino_uno_wrapper_lowering.hex");
    let stdlib_source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("stdlib")
        .join("arduino.rn");
    fs::copy(&stdlib_source, dir.join("arduino.rn")).expect("failed to stage arduino stdlib");

    fs::write(
        &source_path,
        "import arduino\n\n\
         def setup() -> unit:\n    arduino.uart_begin(115200)\n    println(\"Rune AVR started\")\n    return\n\n\
         def loop() -> unit:\n    println(\"tick\")\n    arduino.delay_ms(1000)\n    return\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno wrapper lowering build should succeed");

    let bytes = fs::read(&output_path).expect("failed to read arduino uno wrapper lowering hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
    assert!(!output_path.with_extension("cpp").exists());
}

#[test]
fn builds_arduino_uno_with_extended_hardware_io() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_hardware_io.rn");
    let output_path = dir.join("arduino_uno_hardware_io.hex");
    let stdlib_source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("stdlib")
        .join("arduino.rn");
    fs::copy(&stdlib_source, dir.join("arduino.rn")).expect("failed to stage arduino stdlib");

    fs::write(
        &source_path,
        "from arduino import analog_ref_default, analog_reference, bit_order_msb_first, high, low, mode_output, no_tone, pin_mode, pulse_in, shift_out, tone\n\n\
         def main() -> i32:\n    let data_pin: i64 = 11\n    let clock_pin: i64 = 13\n    let buzzer_pin: i64 = 8\n    pin_mode(data_pin, mode_output())\n    pin_mode(clock_pin, mode_output())\n    analog_reference(analog_ref_default())\n    shift_out(data_pin, clock_pin, bit_order_msb_first(), 170)\n    let duration: i64 = pulse_in(clock_pin, true, 1000)\n    if duration >= 0:\n        println(high())\n        println(low())\n    tone(buzzer_pin, 880, 10)\n    no_tone(buzzer_pin)\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno extended hardware io build should succeed");

    let bytes =
        fs::read(&output_path).expect("failed to read arduino uno extended hardware io hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_with_string_and_int_conversions() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_string_int.rn");
    let output_path = dir.join("arduino_uno_string_int.hex");
    let stdlib_source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("stdlib")
        .join("arduino.rn");
    fs::copy(&stdlib_source, dir.join("arduino.rn")).expect("failed to stage arduino stdlib");

    fs::write(
        &source_path,
        "from arduino import read_line, uart_begin\n\n\
         def main() -> i32:\n    uart_begin(115200)\n    let text: String = str(42)\n    println(text == \"42\")\n    let number: i64 = int(\"123\")\n    println(number + 1)\n    let again: i64 = int(read_line())\n    println(again)\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno string/int conversion build should succeed");

    let bytes =
        fs::read(&output_path).expect("failed to read arduino uno string/int conversion hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_with_for_range_and_sum() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_for_range_sum.rn");
    let output_path = dir.join("arduino_uno_for_range_sum.hex");

    fs::write(
        &source_path,
        "for i in range(5):\n    println(i)\nprintln(sum(range(1, 5)))\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno for/range/sum build should succeed");

    let bytes = fs::read(&output_path).expect("failed to read arduino uno for/range/sum hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_with_sys_platform_detection() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_sys_demo.rn");
    let output_path = dir.join("arduino_uno_sys_demo.hex");

    fs::write(
        &source_path,
        "from sys import platform, arch, target, board, is_embedded, is_wasm\nfrom arduino import delay_ms\n\n\
         def setup() -> unit:\n    println(platform())\n    println(arch())\n    println(target())\n    println(board())\n    println(str(is_embedded()))\n    println(str(is_wasm()))\n    return\n\n\
         def loop() -> unit:\n    delay_ms(1000)\n    return\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno sys program should build");

    assert!(output_path.is_file(), "expected HEX output to exist");
}

#[test]
fn builds_arduino_uno_with_class_field_access() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_class_fields.rn");
    let output_path = dir.join("arduino_uno_class_fields.hex");
    let stdlib_source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("stdlib")
        .join("arduino.rn");
    fs::copy(&stdlib_source, dir.join("arduino.rn")).expect("failed to stage arduino stdlib");

    fs::write(
        &source_path,
        "class Point:\n    x: i32\n    y: i32\n\n\
         def main() -> i32:\n    let point: Point = Point(x=20, y=22)\n    println(point.x)\n    println(point.y)\n    println(point.x + point.y)\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno class field build should succeed");

    let bytes = fs::read(&output_path).expect("failed to read arduino uno class field hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_with_class_method_calls() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_class_methods.rn");
    let output_path = dir.join("arduino_uno_class_methods.hex");
    let stdlib_source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("stdlib")
        .join("arduino.rn");
    fs::copy(&stdlib_source, dir.join("arduino.rn")).expect("failed to stage arduino stdlib");

    fs::write(
        &source_path,
        "class Point:\n    x: i32\n    y: i32\n    def sum(self) -> i32:\n        return self.x + self.y\n\n\
         def main() -> i32:\n    let point: Point = Point(x=20, y=22)\n    println(point.sum())\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno class method build should succeed");

    let bytes = fs::read(&output_path).expect("failed to read arduino uno class method hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_with_class_method_calls_using_self_fields() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_class_method_self.rn");
    let output_path = dir.join("arduino_uno_class_method_self.hex");
    let stdlib_source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("stdlib")
        .join("arduino.rn");
    fs::copy(&stdlib_source, dir.join("arduino.rn")).expect("failed to stage arduino stdlib");

    fs::write(
        &source_path,
        "class Point:\n    x: i32\n    y: i32\n    def sum(self) -> i32:\n        return self.x + self.y\n\n\
         def main() -> i32:\n    let point: Point = Point(x=20, y=22)\n    println(point.sum())\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno class method self build should succeed");

    let bytes = fs::read(&output_path).expect("failed to read arduino uno class method self hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_with_object_returning_and_object_accepting_methods() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_oop_combo.rn");
    let output_path = dir.join("arduino_uno_oop_combo.hex");
    let stdlib_source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("stdlib")
        .join("arduino.rn");
    fs::copy(&stdlib_source, dir.join("arduino.rn")).expect("failed to stage arduino stdlib");

    fs::write(
        &source_path,
        "class Counter:\n    value: i32\n    def bump(self) -> Counter:\n        return Counter(value=self.value + 1)\n    def add(self, other: Counter) -> i32:\n        return self.value + other.value\n\n\
         def main() -> i32:\n    let left: Counter = Counter(value=4)\n    let right: Counter = Counter(value=8)\n    let next: Counter = left.bump()\n    println(next.value)\n    println(left.add(right))\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno object-returning method build should succeed");

    let bytes = fs::read(&output_path).expect("failed to read arduino uno oop combo hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_with_keyword_args_for_helper_functions() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_keyword_helpers.rn");
    let output_path = dir.join("arduino_uno_keyword_helpers.hex");
    let stdlib_source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("stdlib")
        .join("arduino.rn");
    fs::copy(&stdlib_source, dir.join("arduino.rn")).expect("failed to stage arduino stdlib");

    fs::write(
        &source_path,
        "def add_scaled(left: i32, right: i32, scale: i32) -> i32:\n    return (left + right) * scale\n\n\
         def main() -> i32:\n    println(add_scaled(left=4, right=8, scale=2))\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno keyword helper arg build should succeed");

    let bytes = fs::read(&output_path).expect("failed to read arduino uno keyword helper hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_with_keyword_args_for_methods() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_keyword_methods.rn");
    let output_path = dir.join("arduino_uno_keyword_methods.hex");
    let stdlib_source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("stdlib")
        .join("arduino.rn");
    fs::copy(&stdlib_source, dir.join("arduino.rn")).expect("failed to stage arduino stdlib");

    fs::write(
        &source_path,
        "class Mixer:\n    base: i32\n    def combine(self, left: i32, right: i32) -> i32:\n        return self.base + left + right\n\n\
         def main() -> i32:\n    let mixer = Mixer(base=10)\n    println(mixer.combine(right=8, left=4))\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno keyword method arg build should succeed");

    let bytes = fs::read(&output_path).expect("failed to read arduino uno keyword method hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_with_inline_constructor_keyword_method_calls() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_inline_keyword_methods.rn");
    let output_path = dir.join("arduino_uno_inline_keyword_methods.hex");
    let stdlib_source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("stdlib")
        .join("arduino.rn");
    fs::copy(&stdlib_source, dir.join("arduino.rn")).expect("failed to stage arduino stdlib");

    fs::write(
        &source_path,
        "class Mixer:\n    base: i32\n    def combine(self, left: i32, right: i32) -> i32:\n        return self.base + left + right\n\n\
         def main() -> i32:\n    println(Mixer(base=10).combine(right=8, left=4))\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno inline keyword method build should succeed");

    let bytes =
        fs::read(&output_path).expect("failed to read arduino uno inline keyword method hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_with_str_magic_method() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_str_magic.rn");
    let output_path = dir.join("arduino_uno_str_magic.hex");
    let stdlib_source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("stdlib")
        .join("arduino.rn");
    fs::copy(&stdlib_source, dir.join("arduino.rn")).expect("failed to stage arduino stdlib");

    fs::write(
        &source_path,
        "class Counter:\n    value: i32\n    def __str__(self) -> String:\n        return \"Counter(\" + str(self.value) + \")\"\n\n\
         def main() -> i32:\n    println(str(Counter(value=5)))\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno str magic method build should succeed");

    let bytes = fs::read(&output_path).expect("failed to read arduino uno str magic hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_with_repr_magic_method() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_repr_magic.rn");
    let output_path = dir.join("arduino_uno_repr_magic.hex");
    let stdlib_source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("stdlib")
        .join("arduino.rn");
    fs::copy(&stdlib_source, dir.join("arduino.rn")).expect("failed to stage arduino stdlib");

    fs::write(
        &source_path,
        "class Counter:\n    value: i32\n    def __repr__(self) -> String:\n        return \"Counter<value=\" + str(self.value) + \">\"\n\n\
         def main() -> i32:\n    println(repr(Counter(value=5)))\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno repr magic method build should succeed");

    let bytes = fs::read(&output_path).expect("failed to read arduino uno repr magic hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_with_default_object_string() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_default_object_str.rn");
    let output_path = dir.join("arduino_uno_default_object_str.hex");
    let stdlib_source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("stdlib")
        .join("arduino.rn");
    fs::copy(&stdlib_source, dir.join("arduino.rn")).expect("failed to stage arduino stdlib");

    fs::write(
        &source_path,
        "class Counter:\n    value: i32\n\n\
         def main() -> i32:\n    println(str(Counter(value=5)))\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno default object string build should succeed");

    let bytes =
        fs::read(&output_path).expect("failed to read arduino uno default object string hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_with_direct_print_object() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_direct_print_object.rn");
    let output_path = dir.join("arduino_uno_direct_print_object.hex");
    let stdlib_source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("stdlib")
        .join("arduino.rn");
    fs::copy(&stdlib_source, dir.join("arduino.rn")).expect("failed to stage arduino stdlib");

    fs::write(
        &source_path,
        "class Counter:\n    value: i32\n\n\
         def main() -> i32:\n    println(Counter(value=5))\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno direct object print build should succeed");

    let bytes = fs::read(&output_path).expect("failed to read arduino uno direct object print hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_with_default_struct_equality() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_default_struct_eq.rn");
    let output_path = dir.join("arduino_uno_default_struct_eq.hex");
    let stdlib_source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("stdlib")
        .join("arduino.rn");
    fs::copy(&stdlib_source, dir.join("arduino.rn")).expect("failed to stage arduino stdlib");

    fs::write(
        &source_path,
        "class Point:\n    x: i32\n    y: i32\n\n\
         def main() -> i32:\n    let left: Point = Point(x=1, y=2)\n    let same: Point = Point(x=1, y=2)\n    let other: Point = Point(x=1, y=3)\n    println(left == same)\n    println(left != other)\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno default struct equality build should succeed");

    let bytes = fs::read(&output_path).expect("failed to read arduino uno default struct equality hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_with_eq_magic_method() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_eq_magic.rn");
    let output_path = dir.join("arduino_uno_eq_magic.hex");
    let stdlib_source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("stdlib")
        .join("arduino.rn");
    fs::copy(&stdlib_source, dir.join("arduino.rn")).expect("failed to stage arduino stdlib");

    fs::write(
        &source_path,
        "class Counter:\n    value: i32\n    def __eq__(self, other: Counter) -> bool:\n        return self.value == other.value\n\n\
         def main() -> i32:\n    let left: Counter = Counter(value=5)\n    let same: Counter = Counter(value=5)\n    let other: Counter = Counter(value=7)\n    println(left == same)\n    println(left != other)\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno __eq__ magic method build should succeed");

    let bytes = fs::read(&output_path).expect("failed to read arduino uno __eq__ magic method hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_with_string_returning_class_method() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_string_method.hex.rn");
    let output_path = dir.join("arduino_uno_string_method.hex");
    let stdlib_source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("stdlib")
        .join("arduino.rn");
    fs::copy(&stdlib_source, dir.join("arduino.rn")).expect("failed to stage arduino stdlib");

    fs::write(
        &source_path,
        "class Greeter:\n    name: String\n    def greet(self) -> String:\n        return \"hi \" + self.name\n\n\
         def main() -> i32:\n    let greeter = Greeter(name=\"Rune\")\n    println(greeter.greet())\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno string-returning method build should succeed");

    let bytes = fs::read(&output_path).expect("failed to read arduino uno string method hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_with_chained_string_method_results() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_string_chain.rn");
    let output_path = dir.join("arduino_uno_string_chain.hex");
    let stdlib_source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("stdlib")
        .join("arduino.rn");
    fs::copy(&stdlib_source, dir.join("arduino.rn")).expect("failed to stage arduino stdlib");

    fs::write(
        &source_path,
        "class Greeter:\n    name: String\n    def greet(self) -> String:\n        return \"hi \" + self.name\n\n\
         def main() -> i32:\n    let greeter = Greeter(name=\"Rune\")\n    println(greeter.greet() + \" / \" + greeter.greet())\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno chained string method build should succeed");

    let bytes =
        fs::read(&output_path).expect("failed to read arduino uno chained string method hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_with_pin_pwm_and_voltage_abstractions() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_hardware_classes.rn");
    let output_path = dir.join("arduino_uno_hardware_classes.hex");
    let stdlib_source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("stdlib")
        .join("arduino.rn");
    fs::copy(&stdlib_source, dir.join("arduino.rn")).expect("failed to stage arduino stdlib");

    fs::write(
        &source_path,
        "from arduino import AnalogPin, DigitalPin, PwmPin, default_reference_mv, led_builtin, pwm_duty_max\n\n\
         def main() -> i32:\n    let led = DigitalPin(pin=led_builtin())\n    let pwm = PwmPin(pin=9)\n    let sensor = AnalogPin(pin=0)\n    led.output()\n    led.high()\n    led.toggle()\n    pwm.output()\n    pwm.write(pwm_duty_max() / 2)\n    println(sensor.read())\n    println(sensor.read_voltage_mv(default_reference_mv()))\n    println(pwm.max_duty())\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno hardware abstraction build should succeed");

    let bytes =
        fs::read(&output_path).expect("failed to read arduino uno hardware abstraction hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_with_pin_pulse_and_analog_percent() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_pin_pulse_percent.rn");
    let output_path = dir.join("arduino_uno_pin_pulse_percent.hex");
    let stdlib_source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("stdlib")
        .join("arduino.rn");
    fs::copy(&stdlib_source, dir.join("arduino.rn")).expect("failed to stage arduino stdlib");

    fs::write(
        &source_path,
        "from arduino import AnalogPin, DigitalPin, led_builtin\n\n\
         def main() -> i32:\n    let led = DigitalPin(pin=led_builtin())\n    let sensor = AnalogPin(pin=0)\n    led.pulse(5, 5)\n    println(sensor.read_percent())\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno pulse/percent build should succeed");

    let bytes = fs::read(&output_path).expect("failed to read arduino uno pulse/percent hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_with_direct_function_pin_io() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_direct_pin_io.rn");
    let output_path = dir.join("arduino_uno_direct_pin_io.hex");
    let stdlib_source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("stdlib")
        .join("arduino.rn");
    fs::copy(&stdlib_source, dir.join("arduino.rn")).expect("failed to stage arduino stdlib");

    fs::write(
        &source_path,
        "from arduino import digital_in, digital_in_pullup, digital_out, pwm_duty_max, pwm_write\n\n\
         def main() -> i32:\n    digital_out(7, false)\n    println(digital_in(8))\n    println(digital_in_pullup(8))\n    pwm_write(9, pwm_duty_max() / 2)\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno direct function pin io build should succeed");

    let bytes =
        fs::read(&output_path).expect("failed to read arduino uno direct function pin io hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_with_direct_analog_input_functions() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_direct_analog_io.rn");
    let output_path = dir.join("arduino_uno_direct_analog_io.hex");
    let stdlib_source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("stdlib")
        .join("arduino.rn");
    fs::copy(&stdlib_source, dir.join("arduino.rn")).expect("failed to stage arduino stdlib");

    fs::write(
        &source_path,
        "from arduino import analog_in, analog_in_percent, analog_in_voltage_mv, default_reference_mv\n\n\
         def main() -> i32:\n    println(analog_in(0))\n    println(analog_in_percent(0))\n    println(analog_in_voltage_mv(0, default_reference_mv()))\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno direct analog input build should succeed");

    let bytes = fs::read(&output_path).expect("failed to read arduino uno direct analog input hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_buzzer_example() {
    let dir = temp_dir();
    let source_path = dir.join("buzzer_arduino.rn");
    let output_path = dir.join("buzzer_arduino.hex");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    fs::copy(root.join("buzzer_arduino.rn"), &source_path).expect("failed to stage buzzer example");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno buzzer example should build");

    let bytes = fs::read(&output_path).expect("failed to read buzzer hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_buzzer_serial_control_example() {
    let dir = temp_dir();
    let source_path = dir.join("buzzer_serial_control_arduino.rn");
    let output_path = dir.join("buzzer_serial_control_arduino.hex");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    fs::copy(root.join("buzzer_serial_control_arduino.rn"), &source_path)
        .expect("failed to stage buzzer serial control example");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno buzzer serial control example should build");

    let bytes = fs::read(&output_path).expect("failed to read buzzer serial control hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_with_gpio_stdlib_surface() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_gpio_surface.rn");
    let output_path = dir.join("arduino_uno_gpio_surface.hex");

    fs::write(
        &source_path,
        "from gpio import analog_pin, gpio_pin, pwm_pin\n\n\
         def main() -> i32:\n    let led = gpio_pin(13)\n    let pwm = pwm_pin(9)\n    let sensor = analog_pin(0)\n    led.output()\n    led.toggle()\n    pwm.output()\n    pwm.write(64)\n    println(sensor.read())\n    println(sensor.read_percent())\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno gpio stdlib build should succeed");

    let bytes = fs::read(&output_path).expect("failed to read gpio stdlib hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_with_builtin_gpio_without_staged_stdlib_files() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_builtin_gpio_only.rn");
    let output_path = dir.join("arduino_uno_builtin_gpio_only.hex");

    fs::write(
        &source_path,
        "from gpio import gpio_pin\n\n\
         def main() -> i32:\n    let led = gpio_pin(13)\n    led.output()\n    led.high()\n    println(led.read())\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno built-in gpio program should build without staged stdlib files");

    let bytes = fs::read(&output_path).expect("failed to read built-in gpio hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_with_gpio_alias_factories() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_gpio_aliases.rn");
    let output_path = dir.join("arduino_uno_gpio_aliases.hex");

    fs::write(
        &source_path,
        "from gpio import analog, pin, pwm\n\n\
         def main() -> i32:\n    let led = pin(13)\n    let pwm_out = pwm(9)\n    let sensor = analog(0)\n    led.output()\n    pwm_out.output()\n    println(sensor.read())\n    println(sensor.read_percent())\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno built-in gpio alias build should succeed");

    let bytes = fs::read(&output_path).expect("failed to read gpio alias hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_with_builtin_gpio_function_surface() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_builtin_gpio_functions.rn");
    let output_path = dir.join("arduino_uno_builtin_gpio_functions.hex");

    fs::write(
        &source_path,
        "from gpio import analog_in, analog_in_percent, digital_in, digital_in_pullup, digital_out, pwm_duty_max, pwm_write\n\n\
         def main() -> i32:\n    digital_out(7, false)\n    println(digital_in(7))\n    println(digital_in_pullup(8))\n    pwm_write(9, pwm_duty_max() / 2)\n    println(analog_in(0))\n    println(analog_in_percent(0))\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno built-in gpio function surface should build");

    let bytes = fs::read(&output_path).expect("failed to read built-in gpio function hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_with_builtin_pwm_and_adc_modules() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_builtin_pwm_adc.rn");
    let output_path = dir.join("arduino_uno_builtin_pwm_adc.hex");

    fs::write(
        &source_path,
        "from pwm import pwm_pin\nfrom adc import adc_pin, max\n\n\
         def main() -> i32:\n    let pwm = pwm_pin(9)\n    let sensor = adc_pin(0)\n    pwm.output()\n    pwm.write(64)\n    println(sensor.read())\n    println(sensor.read_percent())\n    println(sensor.read_voltage_mv(5000))\n    println(pwm.max_duty())\n    println(max())\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno built-in pwm/adc modules should build");

    let bytes = fs::read(&output_path).expect("failed to read built-in pwm/adc hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_with_namespaced_builtin_pwm_and_adc_modules() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_namespaced_builtin_pwm_adc.rn");
    let output_path = dir.join("arduino_uno_namespaced_builtin_pwm_adc.hex");

    fs::write(
        &source_path,
        "import pwm\nimport adc\n\n\
         def main() -> i32:\n    let out = pwm.pin(9)\n    let sensor = adc.pin(0)\n    out.output()\n    out.write(64)\n    println(sensor.read())\n    println(sensor.read_percent())\n    println(out.max_duty())\n    println(adc.max())\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno namespaced built-in pwm/adc modules should build");

    let bytes = fs::read(&output_path).expect("failed to read namespaced built-in pwm/adc hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_servo_serial_control_example() {
    let dir = temp_dir();
    let source_path = dir.join("servo_serial_control_arduino.rn");
    let output_path = dir.join("servo_serial_control_arduino.hex");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    fs::copy(root.join("servo_serial_control_arduino.rn"), &source_path)
        .expect("failed to stage servo serial control example");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno servo serial control example should build");

    let bytes = fs::read(&output_path).expect("failed to read servo serial control hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_servo_angle_control_example() {
    let dir = temp_dir();
    let source_path = dir.join("servo_angle_control_arduino.rn");
    let output_path = dir.join("servo_angle_control_arduino.hex");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    fs::copy(root.join("servo_angle_control_arduino.rn"), &source_path)
        .expect("failed to stage servo angle control example");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno servo angle control example should build");

    let bytes = fs::read(&output_path).expect("failed to read servo angle control hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_with_calibrated_servo_and_range_helpers() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_servo_calibrated_helpers.rn");
    let output_path = dir.join("arduino_uno_servo_calibrated_helpers.hex");
    let stdlib_source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("stdlib")
        .join("arduino.rn");
    fs::copy(&stdlib_source, dir.join("arduino.rn")).expect("failed to stage arduino stdlib");

    fs::write(
        &source_path,
        "from arduino import clamp_i64, digital_toggle, map_range, servo_attach, servo_pulse_for_angle, servo_write_calibrated\n\n\
         def main() -> i32:\n    let servo_pin: i64 = 9\n    let angle: i64 = clamp_i64(200, 0, 180)\n    let pulse_us: i64 = servo_pulse_for_angle(angle, 2000, 1000)\n    if servo_attach(servo_pin):\n        servo_write_calibrated(servo_pin, angle, 2000, 1000)\n    digital_toggle(13)\n    println(map_range(50, 0, 100, 1000, 2000))\n    println(pulse_us)\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno calibrated servo helper build should succeed");

    let bytes = fs::read(&output_path).expect("failed to read calibrated servo helper hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_with_break_and_continue() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_break_continue.rn");
    let output_path = dir.join("arduino_uno_break_continue.hex");

    fs::write(
        &source_path,
        "def main() -> i32:\n    let value = 0\n    while value < 5:\n        value = value + 1\n        if value == 2:\n            continue\n        println(value)\n        if value == 4:\n            break\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno break/continue build should succeed");

    let bytes = fs::read(&output_path).expect("failed to read break/continue hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_with_shift_interrupts_and_random() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_shift_interrupts_random.rn");
    let output_path = dir.join("arduino_uno_shift_interrupts_random.hex");
    let stdlib_source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("stdlib")
        .join("arduino.rn");
    fs::copy(&stdlib_source, dir.join("arduino.rn")).expect("failed to stage arduino stdlib");

    fs::write(
        &source_path,
        "from arduino import (\n    bit_order_msb_first,\n    interrupts_disable,\n    interrupts_enable,\n    random_i64,\n    random_range,\n    random_seed,\n    shift_in,\n)\n\n\
         def main() -> i32:\n    interrupts_disable()\n    random_seed(42)\n    let sampled: i64 = random_i64(10)\n    let ranged: i64 = random_range(5, 9)\n    interrupts_enable()\n    let bits: i64 = shift_in(8, 7, bit_order_msb_first())\n    println(sampled)\n    println(ranged)\n    println(bits)\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno shift/interrupts/random build should succeed");

    let bytes = fs::read(&output_path).expect("failed to read shift/interrupts/random hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_with_runtime_zero_division_guard() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_zero_division_guard.rn");
    let output_path = dir.join("arduino_uno_zero_division_guard.hex");

    fs::write(
        &source_path,
        "def main() -> i32:\n    let value: i64 = 10\n    let zero: i64 = int(\"0\")\n    println(value / zero)\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno zero division guard build should succeed");

    let bytes = fs::read(&output_path).expect("failed to read zero division guard hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_with_serial_flush() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_serial_flush.rn");
    let output_path = dir.join("arduino_uno_serial_flush.hex");

    fs::write(
        &source_path,
        "from serial import begin, flush, send_line\n\n\
         def main() -> i32:\n    begin(115200)\n    send_line(\"ready\")\n    flush()\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno serial flush build should succeed");

    let bytes = fs::read(&output_path).expect("failed to read serial flush hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_with_serial_byte_helpers() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_serial_bytes.rn");
    let output_path = dir.join("arduino_uno_serial_bytes.hex");

    fs::write(
        &source_path,
        "from serial import begin, peek_byte, write_byte\n\n\
         def main() -> i32:\n    begin(115200)\n    println(peek_byte())\n    println(write_byte(65))\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(
        &source_path,
        &output_path,
        Some("avr-atmega328p-arduino-uno"),
    )
    .expect("arduino uno serial byte helper build should succeed");

    let bytes = fs::read(&output_path).expect("failed to read arduino uno serial byte helper hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_and_runs_program_with_c_ffi_on_windows() {
    let _guard = build_lock()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let dir = temp_dir();
    let source_path = dir.join("ffi_call.rn");
    let c_path = dir.join("ffi_add.c");
    let obj_path = dir.join("ffi_add.obj");
    let output_path = dir.join("ffi_call.exe");

    fs::write(
        &source_path,
        "extern def add_from_c(a: i32, b: i32) -> i32\n\n\
         def main() -> i32:\n    return add_from_c(20, 22)\n",
    )
    .expect("failed to write rune source");
    fs::write(&c_path, "int add_from_c(int a, int b) { return a + b; }\n")
        .expect("failed to write c source");

    let clang = rune::toolchain::find_packaged_llvm_tool("clang.exe")
        .expect("packaged clang.exe should exist");
    let compile = std::process::Command::new(clang)
        .arg("--target=x86_64-pc-windows-gnu")
        .arg("-c")
        .arg(&c_path)
        .arg("-o")
        .arg(&obj_path)
        .output()
        .expect("failed to compile c object");
    assert!(
        compile.status.success(),
        "clang stderr: {}",
        String::from_utf8_lossy(&compile.stderr)
    );

    let options = BuildOptions {
        link_search_paths: Vec::new(),
        link_libs: Vec::new(),
        link_args: vec![obj_path.display().to_string()],
        link_c_sources: Vec::new(),
        ..BuildOptions::default()
    };
    build_executable_llvm_with_options(
        &source_path,
        &output_path,
        Some("x86_64-pc-windows-gnu"),
        &options,
    )
    .expect("ffi build should succeed");

    let output = std::process::Command::new(&output_path)
        .output()
        .expect("failed to run ffi executable");
    assert_eq!(output.status.code(), Some(42));
}

#[test]
fn builds_and_runs_program_with_c_string_ffi_on_windows() {
    let _guard = build_lock()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let dir = temp_dir();
    let source_path = dir.join("ffi_string_call.rn");
    let c_path = dir.join("ffi_string.c");
    let obj_path = dir.join("ffi_string.obj");
    let output_path = dir.join("ffi_string_call.exe");

    fs::write(
        &source_path,
        "extern def greet_from_c(name: String) -> String\n\n\
         def main() -> i32:\n    println(greet_from_c(\"Rune\"))\n    return 0\n",
    )
    .expect("failed to write rune source");
    fs::write(
        &c_path,
        "const char* greet_from_c(const char* name) {\n    return (name[0] == 'R' && name[1] == 'u' && name[2] == 'n' && name[3] == 'e' && name[4] == '\\0') ? \"hi from c\" : \"unknown\";\n}\n",
    )
    .expect("failed to write c source");

    let clang = rune::toolchain::find_packaged_llvm_tool("clang.exe")
        .expect("packaged clang.exe should exist");
    let compile = std::process::Command::new(clang)
        .arg("--target=x86_64-pc-windows-gnu")
        .arg("-c")
        .arg(&c_path)
        .arg("-o")
        .arg(&obj_path)
        .output()
        .expect("failed to compile c object");
    assert!(
        compile.status.success(),
        "clang stderr: {}",
        String::from_utf8_lossy(&compile.stderr)
    );

    let options = BuildOptions {
        link_search_paths: Vec::new(),
        link_libs: Vec::new(),
        link_args: vec![obj_path.display().to_string()],
        link_c_sources: Vec::new(),
        ..BuildOptions::default()
    };
    build_executable_llvm_with_options(
        &source_path,
        &output_path,
        Some("x86_64-pc-windows-gnu"),
        &options,
    )
    .expect("ffi string build should succeed");

    let output = std::process::Command::new(&output_path)
        .output()
        .expect("failed to run ffi string executable");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert!(stdout.contains("hi from c"));
}

#[test]
fn builds_linux_program_with_c_string_ffi_via_llvm_backend() {
    let _guard = build_lock()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let dir = temp_dir();
    let source_path = dir.join("ffi_string_linux.rn");
    let c_path = dir.join("ffi_string_linux.c");
    let obj_path = dir.join("ffi_string_linux.o");
    let output_path = dir.join("ffi_string_linux");

    fs::write(
        &source_path,
        "extern def greet_from_c(name: String) -> String\n\n\
         def main() -> i32:\n    println(greet_from_c(\"Rune\"))\n    return 0\n",
    )
    .expect("failed to write rune source");
    fs::write(
        &c_path,
        "const char* greet_from_c(const char* name) {\n    return (name[0] == 'R' && name[1] == 'u' && name[2] == 'n' && name[3] == 'e' && name[4] == '\\0') ? \"hi from c\" : \"unknown\";\n}\n",
    )
    .expect("failed to write c source");

    let clang = rune::toolchain::find_packaged_llvm_tool("clang.exe")
        .expect("packaged clang.exe should exist");
    let compile = std::process::Command::new(clang)
        .arg("--target=x86_64-unknown-linux-gnu")
        .arg("-c")
        .arg(&c_path)
        .arg("-o")
        .arg(&obj_path)
        .output()
        .expect("failed to compile c object");
    assert!(
        compile.status.success(),
        "clang stderr: {}",
        String::from_utf8_lossy(&compile.stderr)
    );

    let options = BuildOptions {
        link_search_paths: Vec::new(),
        link_libs: Vec::new(),
        link_args: vec![obj_path.display().to_string()],
        link_c_sources: Vec::new(),
        ..BuildOptions::default()
    };
    assert_no_zig_linking_gap(build_executable_llvm_with_options(
        &source_path,
        &output_path,
        Some("x86_64-unknown-linux-gnu"),
        &options,
    ));
}

#[test]
fn auto_compiles_c_source_for_linux_ffi_build() {
    let _guard = build_lock()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let dir = temp_dir();
    let source_path = dir.join("ffi_string_linux_auto.rn");
    let c_path = dir.join("ffi_string_linux_auto.c");
    let output_path = dir.join("ffi_string_linux_auto");

    fs::write(
        &source_path,
        "extern def greet_from_c(name: String) -> String\n\n\
         def main() -> i32:\n    println(greet_from_c(\"Rune\"))\n    return 0\n",
    )
    .expect("failed to write rune source");
    fs::write(
        &c_path,
        "const char* greet_from_c(const char* name) {\n    return (name[0] == 'R' && name[1] == 'u' && name[2] == 'n' && name[3] == 'e' && name[4] == '\\0') ? \"hi from c\" : \"unknown\";\n}\n",
    )
    .expect("failed to write c source");

    let options = BuildOptions {
        link_search_paths: Vec::new(),
        link_libs: Vec::new(),
        link_args: Vec::new(),
        link_c_sources: vec![c_path],
        ..BuildOptions::default()
    };
    assert_no_zig_linking_gap(build_executable_llvm_with_options(
        &source_path,
        &output_path,
        Some("x86_64-unknown-linux-gnu"),
        &options,
    ));
}

#[test]
fn builds_macos_program_with_c_string_ffi_via_llvm_backend() {
    let _guard = build_lock()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let dir = temp_dir();
    let source_path = dir.join("ffi_string_macos.rn");
    let c_path = dir.join("ffi_string_macos.c");
    let obj_path = dir.join("ffi_string_macos.o");
    let output_path = dir.join("ffi_string_macos");

    fs::write(
        &source_path,
        "extern def greet_from_c(name: String) -> String\n\n\
         def main() -> i32:\n    println(greet_from_c(\"Rune\"))\n    return 0\n",
    )
    .expect("failed to write rune source");
    fs::write(
        &c_path,
        "const char* greet_from_c(const char* name) {\n    return (name[0] == 'R' && name[1] == 'u' && name[2] == 'n' && name[3] == 'e' && name[4] == '\\0') ? \"hi from c\" : \"unknown\";\n}\n",
    )
    .expect("failed to write c source");

    let clang = rune::toolchain::find_packaged_llvm_tool("clang.exe")
        .expect("packaged clang.exe should exist");
    let compile = std::process::Command::new(clang)
        .arg("--target=x86_64-apple-darwin")
        .arg("-c")
        .arg(&c_path)
        .arg("-o")
        .arg(&obj_path)
        .output()
        .expect("failed to compile c object");
    assert!(
        compile.status.success(),
        "clang stderr: {}",
        String::from_utf8_lossy(&compile.stderr)
    );

    let options = BuildOptions {
        link_search_paths: Vec::new(),
        link_libs: Vec::new(),
        link_args: vec![obj_path.display().to_string()],
        link_c_sources: Vec::new(),
        ..BuildOptions::default()
    };
    assert_no_zig_linking_gap(build_executable_llvm_with_options(
        &source_path,
        &output_path,
        Some("x86_64-apple-darwin"),
        &options,
    ));
}

#[test]
fn builds_and_runs_c_program_against_rune_static_library_on_windows() {
    let _guard = build_lock()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let dir = temp_dir();
    let rune_source_path = dir.join("runeffi.rn");
    let lib_path = dir.join("runeffi.lib");
    let header_path = dir.join("runeffi.h");
    let c_path = dir.join("use_rune.c");
    let obj_path = dir.join("use_rune.obj");
    let exe_path = dir.join("use_rune.exe");

    fs::write(
        &rune_source_path,
        "def add(a: i32, b: i32) -> i32:\n    return a + b\n\n\
         def mul(a: i32, b: i32) -> i32:\n    return a * b\n",
    )
    .expect("failed to write rune library source");

    build_static_library(&rune_source_path, &lib_path, Some("x86_64-pc-windows-gnu"))
        .expect("rune static library build should succeed");

    assert!(lib_path.is_file(), "expected rune static library to exist");
    assert!(
        header_path.is_file(),
        "expected generated rune C header to exist"
    );

    fs::write(
        &c_path,
        "#include \"runeffi.h\"\n\nint main(void) {\n    return mul(6, 7);\n}\n",
    )
    .expect("failed to write c consumer source");

    let assets = rune::toolchain::detect_windows_dev_assets()
        .expect("windows dev assets should be available for the c consumer test");
    let clang_cl = rune::toolchain::find_packaged_llvm_tool("clang-cl.exe")
        .expect("packaged clang-cl.exe should exist");
    let compile = std::process::Command::new(clang_cl)
        .arg("/c")
        .arg("/I")
        .arg(&dir)
        .arg(&c_path)
        .arg(format!("/Fo:{}", obj_path.display()))
        .output()
        .expect("failed to compile c consumer");
    assert!(
        compile.status.success(),
        "clang-cl stderr: {}",
        String::from_utf8_lossy(&compile.stderr)
    );

    let lld_link = rune::toolchain::find_packaged_llvm_tool("lld-link.exe")
        .expect("packaged lld-link.exe should exist");
    let link = std::process::Command::new(lld_link)
        .arg(format!("/out:{}", exe_path.display()))
        .arg(&obj_path)
        .arg(&lib_path)
        .arg(format!("/libpath:{}", assets.msvc_lib_x64.display()))
        .arg(format!("/libpath:{}", assets.sdk_lib_ucrt_x64.display()))
        .arg(format!("/libpath:{}", assets.sdk_lib_um_x64.display()))
        .arg("libcmt.lib")
        .arg("oldnames.lib")
        .arg("kernel32.lib")
        .arg("user32.lib")
        .output()
        .expect("failed to link c consumer");
    assert!(
        link.status.success(),
        "lld-link stderr: {}",
        String::from_utf8_lossy(&link.stderr)
    );

    let output = std::process::Command::new(&exe_path)
        .output()
        .expect("failed to run c consumer");
    assert_eq!(output.status.code(), Some(42));
}
