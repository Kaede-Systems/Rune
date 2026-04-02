use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsDevAssets {
    pub msvc_root: PathBuf,
    pub msvc_include: PathBuf,
    pub msvc_lib_x64: PathBuf,
    pub msvc_lib_arm64: Option<PathBuf>,
    pub sdk_include_ucrt: PathBuf,
    pub sdk_include_um: PathBuf,
    pub sdk_lib_ucrt_x64: PathBuf,
    pub sdk_lib_um_x64: PathBuf,
    pub sdk_lib_ucrt_arm64: Option<PathBuf>,
    pub sdk_lib_um_arm64: Option<PathBuf>,
}

pub fn find_packaged_llvm_tool(tool_name: &str) -> Option<PathBuf> {
    let candidate_names = llvm_tool_candidate_names(tool_name);
    for llvm_root in bundled_llvm_roots() {
        if let Ok(entries) = fs::read_dir(&llvm_root) {
            for entry in entries.flatten() {
                for tool_name in &candidate_names {
                    let candidate = entry.path().join("bin").join(tool_name);
                    if candidate.is_file() {
                        return Some(candidate);
                    }
                }
            }
        }
    }

    None
}

pub fn find_packaged_lld_link() -> Option<PathBuf> {
    find_packaged_llvm_tool("lld-link")
}

pub fn find_packaged_ld_lld() -> Option<PathBuf> {
    find_packaged_llvm_tool("ld.lld")
}

pub fn find_packaged_ld64_lld() -> Option<PathBuf> {
    find_packaged_llvm_tool("ld64.lld")
}

pub fn find_packaged_wasm_ld() -> Option<PathBuf> {
    find_packaged_llvm_tool("wasm-ld")
}

pub fn find_packaged_llvm_cbe() -> Option<PathBuf> {
    let mut roots = Vec::new();

    if let Some(exe_path) = std::env::current_exe().ok() {
        if let Some(bin_dir) = exe_path.parent()
            && let Some(prefix) = bin_dir.parent()
        {
            let installed_root = prefix.join("share").join("rune").join("tools").join("llvm-cbe");
            append_llvm_cbe_roots(&mut roots, &installed_root);
        }
    }

    if let Some(cwd) = std::env::current_dir().ok() {
        for ancestor in cwd.ancestors() {
            append_llvm_cbe_roots(&mut roots, &ancestor.join("tools").join("llvm-cbe"));
        }
    }

    if let Some(exe_path) = std::env::current_exe().ok() {
        for ancestor in exe_path.ancestors() {
            append_llvm_cbe_roots(&mut roots, &ancestor.join("tools").join("llvm-cbe"));
        }
    }

    for root in dedupe_paths(roots) {
        for candidate in [
            root.join("bin").join("llvm-cbe.exe"),
            root.join("bin").join("llvm-cbe"),
            root.join("build-msvc").join("tools").join("llvm-cbe").join("Release").join("llvm-cbe.exe"),
            root.join("build").join("tools").join("llvm-cbe").join("llvm-cbe"),
            root.join("build").join("tools").join("llvm-cbe").join("Release").join("llvm-cbe.exe"),
        ] {
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    None
}

pub fn find_packaged_wasmtime() -> Option<PathBuf> {
    for root in bundled_wasmtime_roots() {
        if let Ok(entries) = fs::read_dir(&root) {
            for entry in entries.flatten() {
                for file_name in ["wasmtime.exe", "wasmtime"] {
                    let candidate = entry.path().join(file_name);
                    if candidate.is_file() {
                        return Some(candidate);
                    }
                }
            }
        }
        for file_name in ["wasmtime.exe", "wasmtime"] {
            let direct = root.join(file_name);
            if direct.is_file() {
                return Some(direct);
            }
        }
    }

    None
}

pub fn find_arduino_avr_gcc() -> Option<PathBuf> {
    find_arduino_avr_tool(&["avr-gcc.exe", "avr-gcc"])
}

pub fn find_arduino_avr_gpp() -> Option<PathBuf> {
    find_arduino_avr_tool(&["avr-g++.exe", "avr-g++", "avr-c++.exe", "avr-c++"])
}

pub fn find_arduino_avr_objcopy() -> Option<PathBuf> {
    find_arduino_avr_tool(&["objcopy.exe", "avr-objcopy.exe", "objcopy", "avr-objcopy"])
}

pub fn find_arduino_avrdude() -> Option<PathBuf> {
    for root in bundled_arduino_avr_roots() {
        for file_name in ["avrdude.exe", "avrdude"] {
            let candidate = root.join("avrdude").join("bin").join(file_name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

pub fn find_arduino_avr_avrdude_conf() -> Option<PathBuf> {
    for root in bundled_arduino_avr_roots() {
        let candidate = root.join("avrdude").join("etc").join("avrdude.conf");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

pub fn find_arduino_avr_core_root() -> Option<PathBuf> {
    for root in bundled_arduino_avr_roots() {
        let candidate = root.join("arduino-core");
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    None
}

pub fn find_arduino_avr_runtime_header() -> Option<PathBuf> {
    for root in bundled_arduino_avr_roots() {
        let candidate = root.join("runtime").join("rune_arduino_runtime.hpp");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

pub fn detect_windows_dev_assets() -> Option<WindowsDevAssets> {
    let msvc_root = newest_child_dir_with_subdir(
        Path::new(r"C:\Program Files\Microsoft Visual Studio\18\Community\VC\Tools\MSVC"),
        "include",
    )?;
    let sdk_root = newest_child_dir(Path::new(r"C:\Program Files (x86)\Windows Kits\10\Lib"))?;
    let sdk_include_root =
        newest_child_dir(Path::new(r"C:\Program Files (x86)\Windows Kits\10\Include"))?;

    let msvc_include = msvc_root.join("include");
    let msvc_lib_x64 = msvc_root.join("lib").join("x64");
    let msvc_lib_arm64 = msvc_root.join("lib").join("arm64");
    let sdk_include_ucrt = sdk_include_root.join("ucrt");
    let sdk_include_um = sdk_include_root.join("um");
    let sdk_lib_ucrt_x64 = sdk_root.join("ucrt").join("x64");
    let sdk_lib_um_x64 = sdk_root.join("um").join("x64");
    let sdk_lib_ucrt_arm64 = sdk_root.join("ucrt").join("arm64");
    let sdk_lib_um_arm64 = sdk_root.join("um").join("arm64");

    let required = [
        &msvc_include,
        &msvc_lib_x64,
        &sdk_include_ucrt,
        &sdk_include_um,
        &sdk_lib_ucrt_x64,
        &sdk_lib_um_x64,
    ];
    if required.iter().all(|path| path.is_dir()) {
        Some(WindowsDevAssets {
            msvc_root,
            msvc_include,
            msvc_lib_x64,
            msvc_lib_arm64: msvc_lib_arm64.is_dir().then_some(msvc_lib_arm64),
            sdk_include_ucrt,
            sdk_include_um,
            sdk_lib_ucrt_x64,
            sdk_lib_um_x64,
            sdk_lib_ucrt_arm64: sdk_lib_ucrt_arm64.is_dir().then_some(sdk_lib_ucrt_arm64),
            sdk_lib_um_arm64: sdk_lib_um_arm64.is_dir().then_some(sdk_lib_um_arm64),
        })
    } else {
        None
    }
}

fn bundled_llvm_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let host_bundle_dirs = host_llvm_bundle_dirs();

    if let Some(exe_path) = std::env::current_exe().ok() {
        if let Some(bin_dir) = exe_path.parent()
            && let Some(prefix) = bin_dir.parent()
        {
            let installed_root = prefix.join("share").join("rune").join("tools").join("llvm21");
            append_llvm_bundle_roots(&mut roots, &installed_root, &host_bundle_dirs);
        }
    }

    if let Some(cwd) = std::env::current_dir().ok() {
        for ancestor in cwd.ancestors() {
            let llvm_root = ancestor.join("tools").join("llvm21");
            append_llvm_bundle_roots(&mut roots, &llvm_root, &host_bundle_dirs);
        }
    }

    if let Some(exe_path) = std::env::current_exe().ok() {
        for ancestor in exe_path.ancestors() {
            let llvm_root = ancestor.join("tools").join("llvm21");
            append_llvm_bundle_roots(&mut roots, &llvm_root, &host_bundle_dirs);
        }
    }

    dedupe_paths(roots)
}

fn bundled_wasmtime_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Some(exe_path) = std::env::current_exe().ok() {
        if let Some(bin_dir) = exe_path.parent()
            && let Some(prefix) = bin_dir.parent()
        {
            let installed_root = prefix.join("share").join("rune").join("tools").join("wasmtime");
            if installed_root.is_dir() {
                roots.push(installed_root.clone());
                roots.push(installed_root.join("extract"));
            }
        }
    }

    if let Some(cwd) = std::env::current_dir().ok() {
        for ancestor in cwd.ancestors() {
            let tools_root = ancestor.join("tools").join("wasmtime");
            if tools_root.is_dir() {
                roots.push(tools_root.clone());
                roots.push(tools_root.join("extract"));
            }
        }
    }

    if let Some(exe_path) = std::env::current_exe().ok() {
        for ancestor in exe_path.ancestors() {
            let tools_root = ancestor.join("tools").join("wasmtime");
            if tools_root.is_dir() {
                roots.push(tools_root.clone());
                roots.push(tools_root.join("extract"));
            }
        }
    }

    dedupe_paths(roots)
}

fn bundled_arduino_avr_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Some(exe_path) = std::env::current_exe().ok() {
        if let Some(bin_dir) = exe_path.parent()
            && let Some(prefix) = bin_dir.parent()
        {
            let installed_root = prefix.join("share").join("rune").join("tools").join("arduino-avr");
            append_arduino_avr_roots(&mut roots, &installed_root);
        }
    }

    if let Some(cwd) = std::env::current_dir().ok() {
        for ancestor in cwd.ancestors() {
            append_arduino_avr_roots(&mut roots, &ancestor.join("tools").join("arduino-avr"));
        }
    }

    if let Some(exe_path) = std::env::current_exe().ok() {
        for ancestor in exe_path.ancestors() {
            append_arduino_avr_roots(&mut roots, &ancestor.join("tools").join("arduino-avr"));
        }
    }

    dedupe_paths(roots)
}

fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut unique = Vec::new();
    for path in paths {
        if !unique.iter().any(|existing| existing == &path) {
            unique.push(path);
        }
    }
    unique
}

fn llvm_tool_candidate_names(tool_name: &str) -> Vec<String> {
    let mut names = Vec::new();
    names.push(tool_name.to_string());

    if let Some(stripped) = tool_name.strip_suffix(".exe") {
        names.push(stripped.to_string());
    } else {
        names.push(format!("{tool_name}.exe"));
    }

    dedupe_strings(names)
}

fn dedupe_strings(values: Vec<String>) -> Vec<String> {
    let mut unique = Vec::new();
    for value in values {
        if !unique.iter().any(|existing| existing == &value) {
            unique.push(value);
        }
    }
    unique
}

fn append_llvm_bundle_roots(roots: &mut Vec<PathBuf>, base_root: &Path, host_bundle_dirs: &[String]) {
    if !base_root.is_dir() {
        return;
    }

    for bundle_dir in host_bundle_dirs {
        let candidate = base_root.join(bundle_dir);
        if candidate.is_dir() {
            roots.push(candidate);
        }
    }

    roots.push(base_root.to_path_buf());
}

fn append_arduino_avr_roots(roots: &mut Vec<PathBuf>, base_root: &Path) {
    if !base_root.is_dir() {
        return;
    }

    let host_dir = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => Some("windows-x64"),
        ("linux", "x86_64") => Some("linux-x64"),
        ("linux", "aarch64") => Some("linux-arm64"),
        ("macos", "x86_64") => Some("macos-x64"),
        ("macos", "aarch64") => Some("macos-arm64"),
        _ => None,
    };

    if let Some(host_dir) = host_dir {
        let candidate = base_root.join(host_dir);
        if candidate.is_dir() {
            roots.push(candidate);
        }
    }

    roots.push(base_root.to_path_buf());
}

fn append_llvm_cbe_roots(roots: &mut Vec<PathBuf>, base_root: &Path) {
    if !base_root.is_dir() {
        return;
    }

    let host_dir = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => Some("windows-x64"),
        ("linux", "x86_64") => Some("linux-x64"),
        ("linux", "aarch64") => Some("linux-arm64"),
        ("macos", "x86_64") => Some("macos-x64"),
        ("macos", "aarch64") => Some("macos-arm64"),
        _ => None,
    };

    if let Some(host_dir) = host_dir {
        let candidate = base_root.join(host_dir);
        if candidate.is_dir() {
            roots.push(candidate);
        }
    }

    roots.push(base_root.to_path_buf());
}

fn host_llvm_bundle_dirs() -> Vec<String> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let mut names = Vec::new();

    match (os, arch) {
        ("windows", "x86_64") => {
            names.push("windows-x64".to_string());
            names.push("x86_64-pc-windows-msvc".to_string());
            names.push("x86_64-pc-windows-gnu".to_string());
        }
        ("windows", "aarch64") => {
            names.push("windows-arm64".to_string());
            names.push("aarch64-pc-windows-msvc".to_string());
            names.push("aarch64-pc-windows-gnu".to_string());
        }
        ("linux", "x86_64") => {
            names.push("linux-x64".to_string());
            names.push("x86_64-unknown-linux-gnu".to_string());
        }
        ("linux", "aarch64") => {
            names.push("linux-arm64".to_string());
            names.push("aarch64-unknown-linux-gnu".to_string());
        }
        ("macos", "x86_64") => {
            names.push("macos-x64".to_string());
            names.push("x86_64-apple-darwin".to_string());
        }
        ("macos", "aarch64") => {
            names.push("macos-arm64".to_string());
            names.push("aarch64-apple-darwin".to_string());
        }
        _ => {}
    }

    dedupe_strings(names)
}

fn newest_child_dir(root: &Path) -> Option<PathBuf> {
    let mut dirs = fs::read_dir(root)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    dirs.sort();
    dirs.pop()
}

fn find_arduino_avr_tool(names: &[&str]) -> Option<PathBuf> {
    for root in bundled_arduino_avr_roots() {
        for bin_dir in [root.join("avr-gcc").join("bin"), root.join("avr-gcc").join("avr").join("bin")] {
            if !bin_dir.is_dir() {
                continue;
            }
            for name in names {
                let candidate = bin_dir.join(name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }

    None
}

fn newest_child_dir_with_subdir(root: &Path, required_subdir: &str) -> Option<PathBuf> {
    let mut dirs = fs::read_dir(root)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && path.join(required_subdir).is_dir())
        .collect::<Vec<_>>();
    dirs.sort();
    dirs.pop()
}
