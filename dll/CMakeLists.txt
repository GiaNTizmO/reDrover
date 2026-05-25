cmake_minimum_required(VERSION 3.20)

# Required for CMAKE_MSVC_RUNTIME_LIBRARY to take effect.
if(POLICY CMP0091)
    cmake_policy(SET CMP0091 NEW)
endif()

project(redrover_native LANGUAGES CXX)

# Static C runtime. The DLL gets injected into Discord's process at load
# time, and we cannot rely on msvcp140.dll / vcruntime140.dll being
# installed on the user's machine. With dynamic CRT, missing redist =>
# the loader silently fails our DLL and falls back to System32\version.dll
# (the exact symptom we hit during initial deployment). Static linkage
# bloats the binary by ~150 KB but guarantees the DLL is self-contained.
set(CMAKE_MSVC_RUNTIME_LIBRARY "MultiThreaded$<$<CONFIG:Debug>:Debug>")

# ---------------------------------------------------------------------------
# Targets:
#   - redrover_core       : static library, portable. The "what to do" logic
#                            (config, proxy URL parsing, UDP strategies,
#                            SOCKS5 framing, socket bookkeeping, logging).
#   - redrover_dll        : Windows only. Produces `version.dll` for DLL
#                            hijacking next to Discord.exe. Uses MinHook.
#   - redrover_preload    : Linux / macOS only. Produces libredrover_preload.so
#                            (or .dylib). Used via LD_PRELOAD /
#                            DYLD_INSERT_LIBRARIES; intercepts socket(2) /
#                            sendto(2) via dlsym(RTLD_NEXT, ...).
#
# Build, Windows (Visual Studio; use Win32 only for a legacy 32-bit Discord):
#     cmake -S . -B build -A x64
#     cmake --build build --config Release
#
# Build, Linux:
#     cmake -S . -B build -DCMAKE_BUILD_TYPE=Release
#     cmake --build build
#
# Build, macOS (Apple clang or Homebrew clang/llvm):
#     cmake -S . -B build -DCMAKE_BUILD_TYPE=Release
#     cmake --build build
# ---------------------------------------------------------------------------

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

if(NOT CMAKE_POSITION_INDEPENDENT_CODE)
    set(CMAKE_POSITION_INDEPENDENT_CODE ON)
endif()

# ---------------------------------------------------------------------------
# redrover_core (cross-platform static library)
# ---------------------------------------------------------------------------
add_library(redrover_core STATIC
    src/platform.cpp
    src/config.cpp
    src/proxy_value.cpp
    src/socket_manager.cpp
    src/udp_strategy.cpp
    src/socks5_udp.cpp
    src/logging.cpp
    src/real_io.cpp
)

target_include_directories(redrover_core PUBLIC src)

if(MSVC)
    target_compile_options(redrover_core PRIVATE /W4 /permissive- /utf-8)
    target_compile_definitions(redrover_core PRIVATE
        _WIN32_WINNT=0x0601 WIN32_LEAN_AND_MEAN NOMINMAX
        _CRT_SECURE_NO_WARNINGS
    )
else()
    target_compile_options(redrover_core PRIVATE -Wall -Wextra -Wpedantic)
endif()

if(WIN32)
    target_link_libraries(redrover_core PUBLIC ws2_32)
endif()

# ---------------------------------------------------------------------------
# redrover_dll — Windows-only DLL hijack target (output: version.dll)
# ---------------------------------------------------------------------------
if(WIN32)
    # Recent Discord builds (1.0.9000+) ship a 64-bit Discord.exe. The
    # injected DLL must match. We don't hard-fail on x86 builds because
    # Discord Canary / PTB or old installs may still be 32-bit; pick the
    # CMake architecture (-A x64 or -A Win32) accordingly. Default is x64.
    message(STATUS "redrover_dll: building for ${CMAKE_VS_PLATFORM_NAME} "
                   "(pointer size ${CMAKE_SIZEOF_VOID_P} bytes). "
                   "Use -A x64 for current Discord, -A Win32 only for legacy 32-bit builds.")

    include(FetchContent)
    FetchContent_Declare(
        MinHook
        GIT_REPOSITORY https://github.com/TsudaKageyu/minhook.git
        GIT_TAG        master
    )
    FetchContent_MakeAvailable(MinHook)

    add_library(redrover_dll SHARED
        src/dllmain.cpp
        src/hooks.cpp
        src/discord_dirs.cpp
        src/exports.cpp
    )

    target_link_libraries(redrover_dll PRIVATE
        redrover_core
        minhook
        user32
        shlwapi
    )

    set_target_properties(redrover_dll PROPERTIES
        OUTPUT_NAME "version"
        PREFIX ""
        SUFFIX ".dll"
    )

    if(MSVC)
        target_compile_options(redrover_dll PRIVATE /W4 /permissive- /utf-8)
        target_compile_definitions(redrover_dll PRIVATE
            _WIN32_WINNT=0x0601 WIN32_LEAN_AND_MEAN NOMINMAX
            _CRT_SECURE_NO_WARNINGS
        )
    endif()
endif()

# ---------------------------------------------------------------------------
# redrover_preload — POSIX preload library
# ---------------------------------------------------------------------------
if(UNIX)
    add_library(redrover_preload SHARED
        src/posix_preload.cpp
    )
    target_link_libraries(redrover_preload PRIVATE redrover_core ${CMAKE_DL_LIBS})

    set_target_properties(redrover_preload PROPERTIES
        OUTPUT_NAME "redrover_preload"
        PREFIX "lib"
    )

    if(APPLE)
        # On macOS the loader treats undefined symbols as errors at link
        # time. Allow them because socket()/sendto() resolve at runtime via
        # the host process's libc, not at link time.
        target_link_options(redrover_preload PRIVATE
            "-Wl,-undefined,dynamic_lookup"
        )
        set_target_properties(redrover_preload PROPERTIES
            SUFFIX ".dylib"
        )
    else()
        # Linux: keep ELF symbol export to default (all extern "C" visible).
        target_link_options(redrover_preload PRIVATE
            "-Wl,--no-as-needed"
        )
        set_target_properties(redrover_preload PROPERTIES
            SUFFIX ".so"
        )
    endif()
endif()

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
message(STATUS "redrover_native build configuration:")
message(STATUS "  System:    ${CMAKE_SYSTEM_NAME} ${CMAKE_SYSTEM_PROCESSOR}")
message(STATUS "  Compiler:  ${CMAKE_CXX_COMPILER_ID} ${CMAKE_CXX_COMPILER_VERSION}")
if(WIN32)
    message(STATUS "  Targets:   redrover_core, redrover_dll (-> version.dll)")
elseif(UNIX)
    if(APPLE)
        message(STATUS "  Targets:   redrover_core, redrover_preload (-> libredrover_preload.dylib)")
    else()
        message(STATUS "  Targets:   redrover_core, redrover_preload (-> libredrover_preload.so)")
    endif()
endif()
