// Frontend and language model.
pub mod build;
pub mod builtin_modules;
pub mod ir;
pub mod lexer;
pub mod module_loader;
pub mod parser;
pub mod semantic;

// Backends and build/runtime integration.
pub mod codegen;
pub mod llvm_backend;
pub mod llvm_ir;
pub mod optimize;
pub mod toolchain;

// Diagnostics and versioning.
pub mod diagnostics;
pub mod version;
pub mod warnings;
