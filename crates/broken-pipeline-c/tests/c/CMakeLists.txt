cmake_minimum_required(VERSION 3.20)
project(broken_pipeline_c_tests C)

enable_testing()
set(CMAKE_C_STANDARD 11)
set(CMAKE_C_STANDARD_REQUIRED ON)

if(NOT DEFINED BROKEN_PIPELINE_C_LIB)
  message(FATAL_ERROR "BROKEN_PIPELINE_C_LIB must point to libbroken_pipeline_c.so")
endif()

if(NOT DEFINED BROKEN_PIPELINE_C_INCLUDE_DIR)
  message(FATAL_ERROR "BROKEN_PIPELINE_C_INCLUDE_DIR must point to the C header directory")
endif()

add_library(broken_pipeline_c SHARED IMPORTED GLOBAL)
set_target_properties(broken_pipeline_c PROPERTIES IMPORTED_LOCATION "${BROKEN_PIPELINE_C_LIB}")
target_include_directories(broken_pipeline_c INTERFACE "${BROKEN_PIPELINE_C_INCLUDE_DIR}")
get_filename_component(BROKEN_PIPELINE_C_LIB_DIR "${BROKEN_PIPELINE_C_LIB}" DIRECTORY)

function(add_c_api_test test_name)
  add_executable(${test_name} ${test_name}.c)
  target_link_libraries(${test_name} PRIVATE broken_pipeline_c)
  set_target_properties(${test_name} PROPERTIES BUILD_RPATH "${BROKEN_PIPELINE_C_LIB_DIR}")
  add_test(NAME ${test_name} COMMAND ${test_name})
endfunction()

add_c_api_test(naive_smoke)
add_c_api_test(error_smoke)
