include_directories( ${CMAKE_CURRENT_BINARY_DIR}/.. ${CMAKE_CURRENT_SOURCE_DIR}/.. )

add_llvm_component_library(LLVMCBackendInfo
  CBackendTargetInfo.cpp

  LINK_COMPONENTS
  MC
  ScalarOpts
  Support
  Target

  ADD_TO_COMPONENT
  CBackend
  )

if(MSVC)
  set_property(TARGET LLVMCBackendInfo PROPERTY
    MSVC_RUNTIME_LIBRARY "MultiThreaded$<$<CONFIG:Debug>:Debug>")
endif()
