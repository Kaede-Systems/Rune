use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir() -> std::path::PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should be after epoch")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("rune-cli-avr-precode-test-{stamp}"));
    fs::create_dir_all(&dir).expect("failed to create temp dir");
    dir
}

#[test]
fn emit_avr_precode_command_prints_real_uno_pre_elf_code() {
    let dir = temp_dir();
    let source_path = dir.join("uno_precode.rn");

    fs::write(
        &source_path,
        "println(\"hello avr\")\n",
    )
    .expect("failed to write rune source");

    let output = Command::new(env!("CARGO_BIN_EXE_rune"))
        .arg("emit-avr-precode")
        .arg(&source_path)
        .output()
        .expect("failed to run rune emit-avr-precode");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf-8");
    assert!(stdout.contains("// --- rune_arduino_uno"));
    assert!(
        stdout.contains("rune_rt_print_str")
            || stdout.contains("rune_arduino_uno.cpp")
            || stdout.contains("Serial.write")
            || stdout.contains("rune_serial_write")
    );
    assert!(
        stdout.contains("rune_entry_main")
            || stdout.contains("#define RUNE_ARDUINO_ENTRY_MAIN 1")
            || stdout.contains("void setup()")
            || stdout.contains("int main(")
    );
}

#[test]
fn emit_avr_precode_omits_servo_runtime_when_unused() {
    let dir = temp_dir();
    let source_path = dir.join("uno_no_servo_precode.rn");

    fs::write(&source_path, "println(\"hello avr\")\n")
        .expect("failed to write rune source");

    let output = Command::new(env!("CARGO_BIN_EXE_rune"))
        .arg("emit-avr-precode")
        .arg(&source_path)
        .output()
        .expect("failed to run rune emit-avr-precode");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf-8");
    if stdout.contains("// --- rune_arduino_uno_runtime.cpp ---") {
        assert!(!stdout.contains("#define RUNE_ARDUINO_ENABLE_SERVO 1"));
    }
}

#[test]
fn emit_avr_precode_omits_serial_read_runtime_when_unused() {
    let dir = temp_dir();
    let source_path = dir.join("uno_no_serial_read_precode.rn");

    fs::write(&source_path, "println(\"hello avr\")\n")
        .expect("failed to write rune source");

    let output = Command::new(env!("CARGO_BIN_EXE_rune"))
        .arg("emit-avr-precode")
        .arg(&source_path)
        .output()
        .expect("failed to run rune emit-avr-precode");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf-8");
    if !stdout.contains("// --- rune_arduino_uno_runtime.cpp ---") {
        assert!(!stdout.contains("static char rune_input_buffer[96];"));
        assert!(!stdout.contains("rune_serial_read_line("));
    }
}

#[test]
fn emit_avr_precode_omits_serial_startup_when_unused() {
    let dir = temp_dir();
    let source_path = dir.join("uno_no_serial_startup_precode.rn");

    fs::write(
        &source_path,
        "from arduino import digital_write, led_builtin, mode_output, pin_mode\n\n\
pin_mode(led_builtin(), mode_output())\n\
digital_write(led_builtin(), true)\n",
    )
    .expect("failed to write rune source");

    let output = Command::new(env!("CARGO_BIN_EXE_rune"))
        .arg("emit-avr-precode")
        .arg(&source_path)
        .output()
        .expect("failed to run rune emit-avr-precode");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf-8");
    if !stdout.contains("// --- rune_arduino_uno_runtime.cpp ---") {
        assert!(!stdout.contains("Serial.begin(115200);"));
    }
}

#[test]
fn emit_avr_precode_enables_servo_runtime_when_used() {
    let dir = temp_dir();
    let source_path = dir.join("uno_servo_precode.rn");

    fs::write(
        &source_path,
        "from arduino import servo_attach\n\n\
def main() -> i32:\n    println(servo_attach(9))\n    return 0\n",
    )
    .expect("failed to write rune source");

    let output = Command::new(env!("CARGO_BIN_EXE_rune"))
        .arg("emit-avr-precode")
        .arg(&source_path)
        .output()
        .expect("failed to run rune emit-avr-precode");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf-8");
    if stdout.contains("// --- rune_arduino_uno_runtime.cpp ---") {
        assert!(stdout.contains("#define RUNE_ARDUINO_ENABLE_SERVO 1"));
    } else {
        assert!(stdout.contains("rune_rt_arduino_servo_attach("));
        assert!(stdout.contains("alignas(Servo)"));
    }
}

#[test]
fn emit_avr_precode_works_for_mega_target() {
    let dir = temp_dir();
    let source_path = dir.join("mega_precode.rn");

    fs::write(&source_path, "println(\"hello mega\")\n")
        .expect("failed to write rune source");

    let output = Command::new(env!("CARGO_BIN_EXE_rune"))
        .arg("emit-avr-precode")
        .arg(&source_path)
        .arg("--target")
        .arg("avr-atmega2560-arduino-mega")
        .output()
        .expect("failed to run rune emit-avr-precode");

    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf-8");
    // Board-specific defines injected into the runtime shim
    assert!(
        stdout.contains("RUNE_AVR_BOARD_NAME \"arduino-mega\""),
        "expected RUNE_AVR_BOARD_NAME to be arduino-mega in:\n{stdout}"
    );
    assert!(
        stdout.contains("RUNE_AVR_TARGET_TRIPLE \"avr-atmega2560-arduino-mega\""),
        "expected RUNE_AVR_TARGET_TRIPLE to be avr-atmega2560-arduino-mega in:\n{stdout}"
    );
}

#[test]
fn emit_avr_precode_works_for_nano_target() {
    let dir = temp_dir();
    let source_path = dir.join("nano_precode.rn");

    fs::write(&source_path, "println(\"hello nano\")\n")
        .expect("failed to write rune source");

    let output = Command::new(env!("CARGO_BIN_EXE_rune"))
        .arg("emit-avr-precode")
        .arg(&source_path)
        .arg("--target")
        .arg("avr-atmega328p-arduino-nano")
        .output()
        .expect("failed to run rune emit-avr-precode");

    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf-8");
    assert!(
        stdout.contains("RUNE_AVR_BOARD_NAME \"arduino-nano\""),
        "expected RUNE_AVR_BOARD_NAME to be arduino-nano in:\n{stdout}"
    );
    assert!(
        stdout.contains("RUNE_AVR_TARGET_TRIPLE \"avr-atmega328p-arduino-nano\""),
        "expected RUNE_AVR_TARGET_TRIPLE to be avr-atmega328p-arduino-nano in:\n{stdout}"
    );
}

#[test]
fn emit_avr_precode_rejects_non_avr_target() {
    let dir = temp_dir();
    let source_path = dir.join("non_avr.rn");

    fs::write(&source_path, "println(\"hello\")\n")
        .expect("failed to write rune source");

    let output = Command::new(env!("CARGO_BIN_EXE_rune"))
        .arg("emit-avr-precode")
        .arg(&source_path)
        .arg("--target")
        .arg("x86_64-unknown-linux-gnu")
        .output()
        .expect("failed to run rune emit-avr-precode");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("AVR board target") || stderr.contains("emit-avr-precode requires"),
        "expected error about AVR target, got:\n{stderr}"
    );
}
