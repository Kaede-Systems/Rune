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
