use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use rune::build::{
    BuildError, BuildOptions, build_executable, build_executable_llvm,
    build_executable_llvm_with_options, build_object_file, build_shared_library,
    build_static_library, default_library_extension, supported_targets, target_spec,
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
    assert!(
        targets
            .iter()
            .any(|target| target.triple == "x86_64-unknown-linux-gnu")
    );
    assert!(
        targets
            .iter()
            .any(|target| target.triple == "x86_64-apple-darwin")
    );
    assert!(
        targets
            .iter()
            .any(|target| target.triple == "x86_64-pc-windows-gnu")
    );
    assert!(
        targets
            .iter()
            .any(|target| target.triple == "aarch64-pc-windows-gnu")
    );
    assert!(
        targets
            .iter()
            .any(|target| target.triple == "wasm32-unknown-unknown")
    );
    assert!(
        targets
            .iter()
            .any(|target| target.triple == "thumbv6m-none-eabi")
    );
    assert!(
        targets
            .iter()
            .any(|target| target.triple == "riscv32-unknown-elf")
    );
    assert!(
        targets
            .iter()
            .any(|target| target.triple == "avr-atmega328p-arduino-uno")
    );
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

    let embedded =
        target_spec(Some("thumbv6m-none-eabi")).expect("embedded target should resolve");
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
    assert!(
        error
            .to_string()
            .contains("requires a target-aware backend")
    );
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

    build_executable(&source_path, &output_path, Some("avr-atmega328p-arduino-uno"))
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

    build_executable(&source_path, &output_path, Some("avr-atmega328p-arduino-uno"))
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
    let stdlib_source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("stdlib").join("arduino.rn");
    fs::copy(&stdlib_source, dir.join("arduino.rn")).expect("failed to stage arduino stdlib");

    fs::write(
        &source_path,
        "from arduino import pin_mode, digital_write, digital_read, analog_read, delay_ms, millis\n\n\
         def main() -> i32:\n    pin_mode(13, 1)\n    digital_write(13, true)\n    let started = millis()\n    let level = digital_read(13)\n    let analog = analog_read(0)\n    if level:\n        println(analog)\n    delay_ms(1)\n    println(millis() >= started)\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &output_path, Some("avr-atmega328p-arduino-uno"))
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
    let stdlib_source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("stdlib").join("arduino.rn");
    fs::copy(&stdlib_source, dir.join("arduino.rn")).expect("failed to stage arduino stdlib");

    fs::write(
        &source_path,
        "from arduino import pin_mode, analog_write, delay_us, micros, mode_output, led_builtin\n\n\
         def main() -> i32:\n    let led: i64 = led_builtin()\n    let output_mode: i64 = mode_output()\n    pin_mode(led, output_mode)\n    analog_write(led, 128)\n    delay_us(10)\n    println(micros() >= 0)\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &output_path, Some("avr-atmega328p-arduino-uno"))
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
    let stdlib_source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("stdlib").join("arduino.rn");
    fs::copy(&stdlib_source, dir.join("arduino.rn")).expect("failed to stage arduino stdlib");

    fs::write(
        &source_path,
        "from arduino import uart_begin, uart_available, uart_read_byte, uart_write, uart_write_byte\n\n\
         def main() -> i32:\n    uart_begin(115200)\n    uart_write(\"Rune UART ready\")\n    uart_write_byte(10)\n    let available: i64 = uart_available()\n    if available > 0:\n        let value: i64 = uart_read_byte()\n        println(value)\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &output_path, Some("avr-atmega328p-arduino-uno"))
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
    fs::copy(root.join("stdlib").join("arduino.rn"), dir.join("arduino.rn"))
        .expect("failed to stage arduino stdlib");
    fs::copy(root.join("serial_calculator_arduino.rn"), &source_path)
        .expect("failed to stage serial calculator example");

    build_executable(&source_path, &output_path, Some("avr-atmega328p-arduino-uno"))
        .expect("arduino uno serial calculator example should build");

    let bytes = fs::read(&output_path).expect("failed to read serial calculator hex");
    assert!(!bytes.is_empty());
    assert_eq!(bytes[0], b':');
    assert!(output_path.with_extension("elf").is_file());
}

#[test]
fn builds_arduino_uno_with_setup_and_loop_entrypoints() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_uno_setup_loop.rn");
    let output_path = dir.join("arduino_uno_setup_loop.hex");
    let stdlib_source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("stdlib").join("arduino.rn");
    fs::copy(&stdlib_source, dir.join("arduino.rn")).expect("failed to stage arduino stdlib");

    fs::write(
        &source_path,
        "from arduino import delay_ms, digital_write, led_builtin, mode_output, pin_mode\n\n\
         def setup() -> unit:\n    let led: i64 = led_builtin()\n    pin_mode(led, mode_output())\n\n\
         def loop() -> unit:\n    let led: i64 = led_builtin()\n    digital_write(led, true)\n    delay_ms(5)\n    digital_write(led, false)\n    delay_ms(5)\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &output_path, Some("avr-atmega328p-arduino-uno"))
        .expect("arduino uno setup/loop build should succeed");

    let bytes = fs::read(&output_path).expect("failed to read arduino uno setup/loop hex");
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
