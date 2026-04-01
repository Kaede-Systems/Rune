use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::llvm_ir::{LlvmIrError, emit_llvm_ir};
use crate::parser::Program;
use crate::toolchain::find_packaged_llvm_tool;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlvmBackendError {
    pub message: String,
}

impl fmt::Display for LlvmBackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for LlvmBackendError {}

impl From<LlvmIrError> for LlvmBackendError {
    fn from(value: LlvmIrError) -> Self {
        Self {
            message: value.message,
        }
    }
}

pub fn emit_object_file(
    program: &Program,
    target_triple: &str,
    output_path: &Path,
) -> Result<String, LlvmBackendError> {
    let llvm_ir = emit_llvm_ir(program)?;
    emit_object_file_from_ir(&llvm_ir, target_triple, output_path)?;
    Ok(llvm_ir)
}

pub fn emit_assembly_file(
    program: &Program,
    target_triple: &str,
    output_path: &Path,
) -> Result<String, LlvmBackendError> {
    let llvm_ir = emit_llvm_ir(program)?;
    emit_assembly_file_from_ir(&llvm_ir, target_triple, output_path)?;
    Ok(llvm_ir)
}

pub fn emit_object_file_from_ir(
    llvm_ir: &str,
    target_triple: &str,
    output_path: &Path,
) -> Result<(), LlvmBackendError> {
    emit_codegen_artifact_from_ir(llvm_ir, target_triple, output_path, "obj")
}

pub fn emit_assembly_file_from_ir(
    llvm_ir: &str,
    target_triple: &str,
    output_path: &Path,
) -> Result<(), LlvmBackendError> {
    emit_codegen_artifact_from_ir(llvm_ir, target_triple, output_path, "asm")
}

fn emit_codegen_artifact_from_ir(
    llvm_ir: &str,
    target_triple: &str,
    output_path: &Path,
    filetype: &str,
) -> Result<(), LlvmBackendError> {
    let temp_dir = create_temp_dir()?;
    let input_path = temp_dir.join("rune.ll");
    fs::write(&input_path, llvm_ir).map_err(|error| LlvmBackendError {
        message: format!("failed to write temporary LLVM IR file: {error}"),
    })?;

    if let Some(parent) = output_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|error| LlvmBackendError {
            message: format!("failed to create `{}`: {error}", parent.display()),
        })?;
    }

    let input_arg = input_path.to_string_lossy().into_owned();
    let output_arg = output_path.to_string_lossy().into_owned();
    run_llvm_tool(
        "opt",
        vec!["-passes=verify".to_string(), input_arg.clone()],
    )?;
    run_llvm_tool(
        "llc",
        vec![
            format!("-mtriple={target_triple}"),
            format!("-filetype={filetype}"),
            "-O3".to_string(),
            input_arg,
            "-o".to_string(),
            output_arg,
        ],
    )?;

    let _ = fs::remove_file(input_path);
    let _ = fs::remove_dir(temp_dir);
    Ok(())
}

fn run_llvm_tool<S, I>(tool_name: &str, args: I) -> Result<(), LlvmBackendError>
where
    S: AsRef<str>,
    I: IntoIterator<Item = S>,
{
    let tool = find_packaged_llvm_tool(tool_name).ok_or_else(|| LlvmBackendError {
        message: format!("packaged LLVM tool not found: {tool_name}"),
    })?;
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_string())
        .collect::<Vec<_>>();
    let output = Command::new(&tool)
        .args(&args)
        .output()
        .map_err(|error| LlvmBackendError {
            message: format!("failed to run `{}`: {error}", tool.display()),
        })?;

    if output.status.success() {
        return Ok(());
    }

    Err(LlvmBackendError {
        message: format!(
            "{} failed with exit code {}{}",
            tool.display(),
            output.status.code().unwrap_or(-1),
            if output.stderr.is_empty() {
                String::new()
            } else {
                format!("\n\n{}", String::from_utf8_lossy(&output.stderr))
            }
        ),
    })
}

fn create_temp_dir() -> Result<PathBuf, LlvmBackendError> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    let dir = env::temp_dir().join(format!("rune-llvm-{stamp}"));
    fs::create_dir_all(&dir).map_err(|error| LlvmBackendError {
        message: format!(
            "failed to create temporary LLVM directory `{}`: {error}",
            dir.display()
        ),
    })?;
    Ok(dir)
}
