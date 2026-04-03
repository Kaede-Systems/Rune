use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use rune::build::build_executable;

fn temp_dir() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should be after epoch")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("rune-runtime-dynamic-test-{stamp}"));
    fs::create_dir_all(&dir).expect("failed to create temp dir");
    dir
}

#[test]
fn builds_and_runs_dynamic_add_program() {
    let dir = temp_dir();
    let source_path = dir.join("dynamic_add.rn");
    let exe_path = dir.join("dynamic_add.exe");

    fs::write(
        &source_path,
        "def main() -> i32:\n    let value = 40\n    value = value + 2\n    println(value)\n    value = value + \"!\"\n    println(value)\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None).expect("dynamic add program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run built executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout, "42\n42!\n");
}

#[test]
fn builds_and_runs_dynamic_comparison_program() {
    let dir = temp_dir();
    let source_path = dir.join("dynamic_cmp.rn");
    let exe_path = dir.join("dynamic_cmp.exe");

    fs::write(
        &source_path,
        "def main() -> i32:\n    let value = 40\n    if value == 40:\n        println(\"eq\")\n    if value < 50:\n        println(\"lt\")\n    value = \"40\"\n    if value == 40:\n        println(\"string-eq\")\n    if value != 99:\n        println(\"ne\")\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None)
        .expect("dynamic comparison program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run built executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout, "eq\nlt\nstring-eq\nne\n");
}

#[test]
fn builds_and_runs_dynamic_numeric_arithmetic_program() {
    let dir = temp_dir();
    let source_path = dir.join("dynamic_math.rn");
    let exe_path = dir.join("dynamic_math.exe");

    fs::write(
        &source_path,
        "def main() -> i32:\n    let value = 10\n    value = value - 3\n    println(value)\n    value = value * 5\n    println(value)\n    value = value / 7\n    println(value)\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None)
        .expect("dynamic numeric arithmetic program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run built executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout, "7\n35\n5\n");
}

#[test]
fn builds_and_runs_boolean_operator_program() {
    let dir = temp_dir();
    let source_path = dir.join("dynamic_logic.rn");
    let exe_path = dir.join("dynamic_logic.exe");

    fs::write(
        &source_path,
        "def main() -> i32:\n    let value = 1\n    value = true\n    if value and not false:\n        println(\"yes\")\n    if false or value:\n        println(\"or\")\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None).expect("dynamic logic program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run built executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout, "yes\nor\n");
}

#[test]
fn builds_and_runs_modulo_program() {
    let dir = temp_dir();
    let source_path = dir.join("modulo_demo.rn");
    let exe_path = dir.join("modulo_demo.exe");

    fs::write(
        &source_path,
        "def rem(a: i32, b: i32) -> i32:\n    return a % b\n\n\
         def main() -> i32:\n    let a: i32 = rem(10, 3)\n    println(a)\n    let value = 10\n    value = true\n    value = 10\n    value = value % 4\n    println(value)\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None).expect("modulo program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run built executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout, "1\n2\n");
}

#[test]
fn builds_and_runs_arduino_random_and_shift_program() {
    let dir = temp_dir();
    let source_path = dir.join("arduino_host_random_shift.rn");
    let exe_path = dir.join("arduino_host_random_shift.exe");

    fs::write(
        &source_path,
        "from arduino import bit_order_msb_first, interrupts_disable, interrupts_enable, random_i64, random_range, random_seed, shift_in\n\n\
         def main() -> i32:\n    interrupts_disable()\n    random_seed(123)\n    let first: i64 = random_i64(10)\n    let second: i64 = random_range(5, 9)\n    interrupts_enable()\n    println(first >= 0 and first < 10)\n    println(second >= 5 and second < 9)\n    println(shift_in(8, 7, bit_order_msb_first()))\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None)
        .expect("arduino random/shift host program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run built executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "true\ntrue\n0\n");
}

#[test]
fn builds_and_runs_panic_program() {
    let dir = temp_dir();
    let source_path = dir.join("panic_demo.rn");
    let exe_path = dir.join("panic_demo.exe");

    fs::write(&source_path, "def main() -> i32:\n    panic(\"boom\")\n")
        .expect("failed to write source");

    build_executable(&source_path, &exe_path, None).expect("panic program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run built executable");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Rune panic: boom"));
    assert!(stderr.contains("panic in main at line 2"));
}

#[test]
fn builds_and_runs_input_program() {
    let dir = temp_dir();
    let source_path = dir.join("input_demo.rn");
    let exe_path = dir.join("input_demo.exe");

    fs::write(
        &source_path,
        "def main() -> i32:\n    println(input())\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None).expect("input program should build");

    let mut child = Command::new(&exe_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to start built executable");

    child
        .stdin
        .as_mut()
        .expect("stdin should be piped")
        .write_all(b"hello rune\n")
        .expect("failed to write stdin");

    let output = child
        .wait_with_output()
        .expect("failed to collect built executable output");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout, "hello rune\n");
}

#[test]
fn builds_and_runs_stderr_output_program() {
    let dir = temp_dir();
    let source_path = dir.join("stderr_demo.rn");
    let exe_path = dir.join("stderr_demo.exe");

    fs::write(
        &source_path,
        "def main() -> i32:\n    eprint(\"warn=\")\n    eprintln(42)\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None).expect("stderr program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run built executable");

    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n");
    assert_eq!(stderr, "warn=42\n");
}

#[test]
fn builds_and_runs_bool_output_program() {
    let dir = temp_dir();
    let source_path = dir.join("bool_output_demo.rn");
    let exe_path = dir.join("bool_output_demo.exe");

    fs::write(
        &source_path,
        "def main() -> i32:\n    println(true)\n    eprintln(false)\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None).expect("bool output program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run built executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    let stderr = String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n");
    assert_eq!(stdout, "true\n");
    assert_eq!(stderr, "false\n");
}

#[test]
fn builds_and_runs_struct_program() {
    let dir = temp_dir();
    let source_path = dir.join("struct_demo.rn");
    let exe_path = dir.join("struct_demo.exe");

    fs::write(
        &source_path,
        "struct Point:\n    x: i32\n    y: i32\n\n\
         def main() -> i32:\n    let point: Point = Point(x=20, y=22)\n    println(point.x)\n    println(point.y)\n    println(point.x + point.y)\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None).expect("struct program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run built executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout, "20\n22\n42\n");
}

#[test]
fn builds_and_runs_struct_parameter_program() {
    let dir = temp_dir();
    let source_path = dir.join("struct_param_demo.rn");
    let exe_path = dir.join("struct_param_demo.exe");

    fs::write(
        &source_path,
        "struct Point:\n    x: i32\n    y: i32\n\n\
         def sum_point(point: Point) -> i32:\n    return point.x + point.y\n\n\
         def main() -> i32:\n    let point: Point = Point(x=20, y=22)\n    println(sum_point(point))\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None).expect("struct parameter program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run built executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout, "42\n");
}

#[test]
fn builds_and_runs_class_method_program() {
    let dir = temp_dir();
    let source_path = dir.join("class_method_demo.rn");
    let exe_path = dir.join("class_method_demo.exe");

    fs::write(
        &source_path,
        "class Point:\n    x: i32\n    y: i32\n    def sum(self) -> i32:\n        return self.x + self.y\n\n\
         def main() -> i32:\n    let point: Point = Point(x=20, y=22)\n    println(point.sum())\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None).expect("class method program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run built executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "42\n");
}

#[test]
fn builds_and_runs_break_continue_program() {
    let dir = temp_dir();
    let source_path = dir.join("break_continue_demo.rn");
    let exe_path = dir.join("break_continue_demo.exe");

    fs::write(
        &source_path,
        "def main() -> i32:\n    let value = 0\n    while value < 5:\n        value = value + 1\n        if value == 2:\n            continue\n        println(value)\n        if value == 4:\n            break\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None).expect("break/continue program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run built executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "1\n3\n4\n");
}

#[test]
fn builds_and_runs_class_method_program_with_keyword_args() {
    let dir = temp_dir();
    let source_path = dir.join("class_method_keywords_demo.rn");
    let exe_path = dir.join("class_method_keywords_demo.exe");

    fs::write(
        &source_path,
        "class Mixer:\n    base: i32\n    def combine(self, left: i32, right: i32) -> i32:\n        return self.base + left + right\n\n\
         def main() -> i32:\n    let mixer: Mixer = Mixer(base=10)\n    println(mixer.combine(right=8, left=4))\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None)
        .expect("class method keyword-arg program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run class method keyword-arg executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "22\n");
}

#[test]
fn builds_and_runs_inline_constructor_method_program_with_keyword_args() {
    let dir = temp_dir();
    let source_path = dir.join("class_inline_method_keywords_demo.rn");
    let exe_path = dir.join("class_inline_method_keywords_demo.exe");

    fs::write(
        &source_path,
        "class Mixer:\n    base: i32\n    def combine(self, left: i32, right: i32) -> i32:\n        return self.base + left + right\n\n\
         def main() -> i32:\n    println(Mixer(base=10).combine(right=8, left=4))\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None)
        .expect("inline constructor method keyword-arg program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run inline constructor method keyword-arg executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "22\n");
}

#[test]
fn builds_and_runs_class_return_program() {
    let dir = temp_dir();
    let source_path = dir.join("class_return_demo.rn");
    let exe_path = dir.join("class_return_demo.exe");

    fs::write(
        &source_path,
        "class Point:\n    x: i32\n    y: i32\n\n\
         def make_point() -> Point:\n    return Point(x=20, y=22)\n\n\
         def main() -> i32:\n    let point: Point = make_point()\n    println(point.x)\n    println(point.y)\n    println(point.x + point.y)\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None).expect("class return program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run built executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "20\n22\n42\n");
}

#[test]
fn builds_and_runs_object_returning_and_object_accepting_method_program() {
    let dir = temp_dir();
    let source_path = dir.join("class_object_method_demo.rn");
    let exe_path = dir.join("class_object_method_demo.exe");

    fs::write(
        &source_path,
        "class Counter:\n    value: i32\n    def bump(self) -> Counter:\n        return Counter(value=self.value + 1)\n    def add(self, other: Counter) -> i32:\n        return self.value + other.value\n\n\
         def main() -> i32:\n    let left: Counter = Counter(value=4)\n    let right: Counter = Counter(value=8)\n    let next: Counter = left.bump()\n    println(next.value)\n    println(left.add(right))\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None)
        .expect("object method program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run object method executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "5\n12\n");
}

#[test]
fn builds_and_runs_string_returning_method_program() {
    let dir = temp_dir();
    let source_path = dir.join("class_string_method_demo.rn");
    let exe_path = dir.join("class_string_method_demo.exe");

    fs::write(
        &source_path,
        "class Greeter:\n    name: String\n    def greet(self) -> String:\n        return \"hi \" + self.name\n\n\
         def main() -> i32:\n    let greeter = Greeter(name=\"Rune\")\n    println(greeter.greet())\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None)
        .expect("string-returning method program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run string-returning method executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "hi Rune\n");
}

#[test]
fn builds_and_runs_str_magic_method_program() {
    let dir = temp_dir();
    let source_path = dir.join("class_str_magic_demo.rn");
    let exe_path = dir.join("class_str_magic_demo.exe");

    fs::write(
        &source_path,
        "class Counter:\n    value: i32\n    def __str__(self) -> String:\n        return \"Counter(\" + str(self.value) + \")\"\n\n\
         def main() -> i32:\n    println(str(Counter(value=5)))\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None)
        .expect("str magic method program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run str magic method executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "Counter(5)\n");
}

#[test]
fn builds_and_runs_default_object_string_program() {
    let dir = temp_dir();
    let source_path = dir.join("class_default_str_demo.rn");
    let exe_path = dir.join("class_default_str_demo.exe");

    fs::write(
        &source_path,
        "class Counter:\n    value: i32\n\n\
         def main() -> i32:\n    println(str(Counter(value=5)))\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None)
        .expect("default object string program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run default object string executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "Counter(value=5)\n");
}

#[test]
fn builds_and_runs_direct_print_object_program() {
    let dir = temp_dir();
    let source_path = dir.join("class_direct_print_demo.rn");
    let exe_path = dir.join("class_direct_print_demo.exe");

    fs::write(
        &source_path,
        "class Counter:\n    value: i32\n\n\
         def main() -> i32:\n    println(Counter(value=5))\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None)
        .expect("direct object print program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run direct object print executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "Counter(value=5)\n");
}

#[test]
fn builds_and_runs_cli_arg_program() {
    let dir = temp_dir();
    let source_path = dir.join("cli_args_demo.rn");
    let exe_path = dir.join("cli_args_demo.exe");

    fs::write(
        &source_path,
        "from env import arg, arg_count\n\n\
         def main() -> i32:\n    println(arg_count())\n    println(arg(0))\n    println(arg(1))\n    println(arg(2))\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None)
        .expect("cli arg program should build");

    let output = Command::new(&exe_path)
        .arg("--port")
        .arg("COM5")
        .output()
        .expect("failed to run cli arg executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "2\n--port\nCOM5\n\n");
}

#[test]
fn builds_and_runs_fs_terminal_and_audio_program() {
    let dir = temp_dir();
    let source_path = dir.join("fs_terminal_audio_demo.rn");
    let exe_path = dir.join("fs_terminal_audio_demo.exe");
    let file_path = dir.join("note.txt");

    fs::write(
        &source_path,
        format!(
            "def main() -> i32:\n    println(__rune_builtin_fs_exists({:?}))\n    println(__rune_builtin_fs_write_string({:?}, \"hello rune\"))\n    println(__rune_builtin_fs_read_string({:?}))\n    __rune_builtin_terminal_clear()\n    __rune_builtin_terminal_move_to(1, 1)\n    __rune_builtin_terminal_hide_cursor()\n    __rune_builtin_terminal_set_title(\"Rune Test\")\n    __rune_builtin_terminal_show_cursor()\n    println(__rune_builtin_audio_bell())\n    return 0\n",
            file_path.to_string_lossy().to_string(),
            file_path.to_string_lossy().to_string(),
            file_path.to_string_lossy().to_string()
        ),
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None)
        .expect("fs/terminal/audio program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run built executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert!(stdout.contains("false\ntrue\nhello rune\n"), "unexpected stdout: {stdout}");
    assert!(stdout.contains("true\n"), "unexpected stdout: {stdout}");
    let file_contents = fs::read_to_string(&file_path).expect("written file should exist");
    assert_eq!(file_contents, "hello rune");
}
