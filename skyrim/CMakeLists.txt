cmake_minimum_required(VERSION 3.25)

project(FaithSkyrim VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 23)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_MSVC_RUNTIME_LIBRARY "MultiThreaded$<$<CONFIG:Debug>:Debug>DLL")

# CommonLibSSE-NG (SE + AE; VR off), the same commit SkyCraft builds against for 1.7.104.
set(BUILD_TESTS OFF CACHE BOOL "" FORCE)
set(ENABLE_SKYRIM_VR OFF CACHE BOOL "" FORCE)
set(SKSE_SUPPORT_XBYAK ON CACHE BOOL "" FORCE)
add_subdirectory(extern/CommonLibSSE-NG CommonLibSSE EXCLUDE_FROM_ALL)
if(NOT COMMAND add_commonlibsse_plugin)
	include(extern/CommonLibSSE-NG/cmake/CommonLibSSE.cmake)
endif()

# Faith's movement and animation: the Rust crate faith_ffi, built with cargo as a static library.
set(FAITH_ROOT "${CMAKE_CURRENT_SOURCE_DIR}/..")
set(FAITH_CARGO_DIR "${CMAKE_BINARY_DIR}/cargo")
set(FAITH_LIB "${FAITH_CARGO_DIR}/release/faith_ffi.lib")
add_custom_target(faith_ffi_cargo
	COMMAND ${CMAKE_COMMAND} -E env CARGO_TARGET_DIR=${FAITH_CARGO_DIR} cargo build -p faith_ffi --release
	WORKING_DIRECTORY "${FAITH_ROOT}"
	BYPRODUCTS "${FAITH_LIB}"
	COMMENT "cargo build -p faith_ffi --release"
	USES_TERMINAL
	VERBATIM)

file(GLOB_RECURSE FAITH_SOURCES CONFIGURE_DEPENDS src/*.cpp src/*.h)

add_commonlibsse_plugin(
	FaithSkyrim
	NAME "FaithSkyrim"
	AUTHOR "faith-runner"
	USE_ADDRESS_LIBRARY
	SOURCES ${FAITH_SOURCES}
)

add_dependencies(FaithSkyrim faith_ffi_cargo)
target_include_directories(FaithSkyrim PRIVATE src "${FAITH_ROOT}/crates/faith_ffi/include" extern/SKSEMenuFramework)
target_precompile_headers(FaithSkyrim PRIVATE src/PCH.h)
target_link_libraries(FaithSkyrim PRIVATE "${FAITH_LIB}" kernel32.lib ntdll.lib userenv.lib ws2_32.lib dbghelp.lib d3d11.lib d3dcompiler.lib)
target_compile_definitions(FaithSkyrim PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN)
if(MSVC)
	target_compile_options(FaithSkyrim PRIVATE /W4 /permissive- /Zc:preprocessor /utf-8 /MP /wd4200)
	target_link_options(FaithSkyrim PRIVATE /DEBUG:FULL)
endif()

# Development: copy the built DLL (and the ini) into a Mod Organizer mod folder when it exists.
if(FAITH_DEPLOY_DIR)
	add_custom_command(TARGET FaithSkyrim POST_BUILD
		COMMAND ${CMAKE_COMMAND} "-DDLL=$<TARGET_FILE:FaithSkyrim>" "-DPDB=$<TARGET_PDB_FILE:FaithSkyrim>" "-DINI=${CMAKE_CURRENT_SOURCE_DIR}/FaithSkyrim.ini" "-DDEST=${FAITH_DEPLOY_DIR}/SKSE/Plugins" -P "${CMAKE_CURRENT_SOURCE_DIR}/cmake/deploy.cmake"
		VERBATIM)
endif()
