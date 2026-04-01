use rune::toolchain::{
    find_packaged_ld_lld, find_packaged_ld64_lld, find_packaged_lld_link, find_packaged_llvm_tool,
    find_packaged_wasm_ld, find_packaged_wasmtime,
};

#[test]
fn finds_packaged_core_llvm_tools() {
    let llc = find_packaged_llvm_tool("llc").expect("packaged llc should exist");
    let llvm_ar = find_packaged_llvm_tool("llvm-ar").expect("packaged llvm-ar should exist");

    assert!(llc.is_file());
    assert!(llvm_ar.is_file());
}

#[test]
fn finds_packaged_linkers() {
    let lld_link = find_packaged_lld_link().expect("packaged lld-link should exist");
    let ld_lld = find_packaged_ld_lld().expect("packaged ld.lld should exist");
    let ld64_lld = find_packaged_ld64_lld().expect("packaged ld64.lld should exist");
    let wasm_ld = find_packaged_wasm_ld().expect("packaged wasm-ld should exist");

    assert!(lld_link.is_file());
    assert!(ld_lld.is_file());
    assert!(ld64_lld.is_file());
    assert!(wasm_ld.is_file());
}

#[test]
fn finds_packaged_wasmtime_binary() {
    let wasmtime = find_packaged_wasmtime().expect("packaged wasmtime binary should exist");
    assert!(wasmtime.is_file());
}
