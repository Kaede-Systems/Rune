# Support plugins.
set(LLVM_NO_DEAD_STRIP 1)

if (NOT USE_SYSTEM_LLVM)
  set(LLVM_LINK_COMPONENTS
    ${LLVM_TARGETS_TO_BUILD}
    Analysis
    AsmParser
    AsmPrinter
    BitReader
    CBackendCodeGen
    CBackendInfo
    CodeGen
    Core
    IRReader
    MC
    ScalarOpts
    SelectionDAG
    Support
    Target
    )


  add_llvm_tool(llvm-cbe
    llvm-cbe.cpp
    )
  export_executable_symbols(llvm-cbe)
endif()

if(USE_SYSTEM_LLVM)
  add_executable(llvm-cbe
    llvm-cbe.cpp
    )
  llvm_map_components_to_libnames(llvm_libs
    CBackendCodeGen
    CBackendInfo
    Analysis
    AArch64AsmParser
    AArch64CodeGen
    AArch64Desc
    AArch64Info
    AArch64Utils
    ARMAsmParser
    ARMCodeGen
    ARMDesc
    ARMInfo
    ARMUtils
    AsmParser
    AsmPrinter
    BitReader
    BPFAsmParser
    BPFCodeGen
    BPFDesc
    BPFInfo
    CodeGen
    Core
    IRReader
    MC
    MCParser
    NVPTXCodeGen
    NVPTXDesc
    NVPTXInfo
    RISCVAsmParser
    RISCVCodeGen
    RISCVDesc
    RISCVInfo
    RISCVTargetMCA
    ScalarOpts
    SelectionDAG
    Support
    Target
    TargetParser
    WebAssemblyAsmParser
    WebAssemblyCodeGen
    WebAssemblyDesc
    WebAssemblyInfo
    WebAssemblyUtils
    X86AsmParser
    X86CodeGen
    X86Desc
    X86Info
    X86TargetMCA
    )
  target_link_libraries(llvm-cbe
    ${llvm_libs})
  if(MSVC)
    set_property(TARGET llvm-cbe PROPERTY
      MSVC_RUNTIME_LIBRARY "MultiThreaded$<$<CONFIG:Debug>:Debug>")
  endif()
endif()
