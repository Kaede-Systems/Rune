use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use rune::build::{
    BuildError, BuildOptions, build_executable, build_executable_llvm,
    build_executable_llvm_with_options, build_shared_library,
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
