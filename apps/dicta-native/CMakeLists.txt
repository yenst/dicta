cmake_minimum_required(VERSION 3.21)

project(dicta_native VERSION 0.8.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)

find_package(Qt6 6.5 REQUIRED COMPONENTS Core Gui Qml Quick)
find_package(Qt6 6.5 QUIET COMPONENTS Multimedia)
find_package(Vulkan REQUIRED)

qt_standard_project_setup(REQUIRES 6.5)
qt_policy(SET QTP0004 NEW)

set(DICTA_RUST_MANIFEST "${CMAKE_CURRENT_SOURCE_DIR}/rust/Cargo.toml")
set(DICTA_WORKSPACE_MANIFEST "${CMAKE_CURRENT_SOURCE_DIR}/../../Cargo.toml")
set(DICTA_RUST_LOCKFILE "${CMAKE_CURRENT_SOURCE_DIR}/../../Cargo.lock")
set(DICTA_RUST_TARGET_DIR "${CMAKE_CURRENT_BINARY_DIR}/cargo-target")
set(DICTA_RUST_LIBRARY "${DICTA_RUST_TARGET_DIR}/release/libdicta_native_bridge.a")
file(GLOB_RECURSE DICTA_RUST_SOURCES CONFIGURE_DEPENDS
    "${CMAKE_CURRENT_SOURCE_DIR}/rust/src/*.rs"
    "${CMAKE_CURRENT_SOURCE_DIR}/../../crates/dicta-capture/src/*.rs"
    "${CMAKE_CURRENT_SOURCE_DIR}/../../crates/dicta-control/src/*.rs"
    "${CMAKE_CURRENT_SOURCE_DIR}/../../crates/dicta-core/src/*.rs"
    "${CMAKE_CURRENT_SOURCE_DIR}/../../crates/dicta-engine/src/*.rs"
    "${CMAKE_CURRENT_SOURCE_DIR}/../../crates/dicta-linux/src/*.rs"
    "${CMAKE_CURRENT_SOURCE_DIR}/../../crates/dicta-overlay/src/*.rs"
    "${CMAKE_CURRENT_SOURCE_DIR}/../../crates/dicta-runtime/src/*.rs"
    "${CMAKE_CURRENT_SOURCE_DIR}/../../crates/dicta-transcribe/src/*.rs"
)
set(DICTA_RUST_MANIFESTS
    "${DICTA_WORKSPACE_MANIFEST}"
    "${DICTA_RUST_MANIFEST}"
    "${CMAKE_CURRENT_SOURCE_DIR}/../../crates/dicta-capture/Cargo.toml"
    "${CMAKE_CURRENT_SOURCE_DIR}/../../crates/dicta-control/Cargo.toml"
    "${CMAKE_CURRENT_SOURCE_DIR}/../../crates/dicta-core/Cargo.toml"
    "${CMAKE_CURRENT_SOURCE_DIR}/../../crates/dicta-engine/Cargo.toml"
    "${CMAKE_CURRENT_SOURCE_DIR}/../../crates/dicta-linux/Cargo.toml"
    "${CMAKE_CURRENT_SOURCE_DIR}/../../crates/dicta-overlay/Cargo.toml"
    "${CMAKE_CURRENT_SOURCE_DIR}/../../crates/dicta-runtime/Cargo.toml"
    "${CMAKE_CURRENT_SOURCE_DIR}/../../crates/dicta-transcribe/Cargo.toml"
)

add_custom_command(
    OUTPUT "${DICTA_RUST_LIBRARY}"
    COMMAND cargo build
            --manifest-path "${DICTA_RUST_MANIFEST}"
            --target-dir "${DICTA_RUST_TARGET_DIR}"
            --release
            --locked
    DEPENDS
        ${DICTA_RUST_MANIFESTS}
        ${DICTA_RUST_SOURCES}
        "${DICTA_RUST_LOCKFILE}"
    COMMENT "Building the Dicta Rust bridge"
    VERBATIM
)

add_custom_target(dicta_native_rust_build ALL DEPENDS "${DICTA_RUST_LIBRARY}")

add_library(dicta_native_rust STATIC IMPORTED GLOBAL)
set_target_properties(dicta_native_rust PROPERTIES
    IMPORTED_LOCATION "${DICTA_RUST_LIBRARY}"
)
add_dependencies(dicta_native_rust dicta_native_rust_build)

qt_add_executable(dicta-native
    src/main.cpp
    src/native_bridge.cpp
    src/native_bridge.h
    src/theme_bridge.cpp
    src/theme_bridge.h
    src/theme_icon_provider.cpp
    src/theme_icon_provider.h
    overlay/overlay_controller.cpp
    overlay/overlay_controller.h
    overlay/overlay_placement.cpp
    overlay/overlay_placement.h
)

qt_add_qml_module(dicta-native
    URI Dicta.Native
    VERSION 1.0
    SOURCES
        overlay/annotation_item.cpp
        overlay/annotation_item.h
    QML_FILES
        qml/Main.qml
        qml/ThemeIcon.qml
        qml/FlatButton.qml
        qml/ProjectRail.qml
        qml/RecordingComposer.qml
        qml/RecordingList.qml
        qml/RecordingDetail.qml
        overlay/AnnotationOverlay.qml
)

qt_add_resources(dicta-native "dicta_brand_assets"
    PREFIX "/dicta/assets"
    BASE "${CMAKE_CURRENT_SOURCE_DIR}/assets"
    FILES
        "${CMAKE_CURRENT_SOURCE_DIR}/assets/dicta-mark.png"
        "${CMAKE_CURRENT_SOURCE_DIR}/assets/dicta-mark-light.png"
)

if(TARGET Qt6::Multimedia)
    qt_target_qml_sources(dicta-native
        QML_FILES qml/RecordingPlayer.qml
    )
    target_link_libraries(dicta-native PRIVATE Qt6::Multimedia)
    target_compile_definitions(dicta-native PRIVATE DICTA_HAS_MULTIMEDIA=1)
endif()

target_include_directories(dicta-native PRIVATE
    "${CMAKE_CURRENT_SOURCE_DIR}/overlay"
)

target_compile_definitions(dicta-native PRIVATE
    DICTA_NATIVE_VERSION="${PROJECT_VERSION}"
)

target_link_libraries(dicta-native PRIVATE
    Qt6::Core
    Qt6::Gui
    Qt6::Qml
    Qt6::Quick
    Vulkan::Vulkan
    dicta_native_rust
)

install(TARGETS dicta-native
    BUNDLE DESTINATION .
    RUNTIME DESTINATION bin
)

option(DICTA_BUILD_TESTS "Build Dicta native tests" OFF)
if(DICTA_BUILD_TESTS)
    enable_testing()
    find_package(Qt6 6.5 REQUIRED COMPONENTS Test)

    qt_add_executable(dicta-overlay-qt-test
        tests/annotation_item_test.cpp
        overlay/annotation_item.cpp
        overlay/annotation_item.h
        overlay/overlay_controller.cpp
        overlay/overlay_controller.h
        overlay/overlay_placement.cpp
        overlay/overlay_placement.h
    )
    target_include_directories(dicta-overlay-qt-test PRIVATE
        "${CMAKE_CURRENT_SOURCE_DIR}/overlay"
    )
    target_compile_definitions(dicta-overlay-qt-test PRIVATE
        DICTA_OVERLAY_QML_PATH="${CMAKE_CURRENT_SOURCE_DIR}/overlay/AnnotationOverlay.qml"
    )
    target_link_libraries(dicta-overlay-qt-test PRIVATE
        Qt6::Core
        Qt6::Gui
        Qt6::Qml
        Qt6::Quick
        Qt6::Test
    )

    add_test(NAME dicta-overlay-qt COMMAND dicta-overlay-qt-test)
    set_tests_properties(dicta-overlay-qt PROPERTIES
        ENVIRONMENT "QT_QPA_PLATFORM=offscreen;QT_QPA_PLATFORMTHEME="
    )

    qt_add_executable(dicta-dashboard-qml-test
        tests/dashboard_qml_test.cpp
        src/theme_icon_provider.cpp
        src/theme_icon_provider.h
    )
    target_include_directories(dicta-dashboard-qml-test PRIVATE
        "${CMAKE_CURRENT_SOURCE_DIR}/src"
    )
    qt_add_resources(dicta-dashboard-qml-test "dicta_dashboard_brand_assets"
        PREFIX "/dicta/assets"
        BASE "${CMAKE_CURRENT_SOURCE_DIR}/assets"
        FILES
            "${CMAKE_CURRENT_SOURCE_DIR}/assets/dicta-mark.png"
            "${CMAKE_CURRENT_SOURCE_DIR}/assets/dicta-mark-light.png"
    )
    target_compile_definitions(dicta-dashboard-qml-test PRIVATE
        DICTA_MAIN_QML_PATH="${CMAKE_CURRENT_SOURCE_DIR}/qml/Main.qml"
    )
    target_link_libraries(dicta-dashboard-qml-test PRIVATE
        Qt6::Core
        Qt6::Gui
        Qt6::Qml
        Qt6::Quick
        Qt6::Test
    )
    add_test(NAME dicta-dashboard-qml COMMAND dicta-dashboard-qml-test)
    set_tests_properties(dicta-dashboard-qml PROPERTIES
        ENVIRONMENT "QT_QPA_PLATFORM=offscreen;QT_QPA_PLATFORMTHEME="
    )

    qt_add_executable(dicta-theme-qt-test
        tests/theme_bridge_test.cpp
        src/theme_bridge.cpp
        src/theme_bridge.h
    )
    target_include_directories(dicta-theme-qt-test PRIVATE
        "${CMAKE_CURRENT_SOURCE_DIR}/src"
    )
    target_link_libraries(dicta-theme-qt-test PRIVATE
        Qt6::Core
        Qt6::Gui
        Qt6::Test
    )
    add_test(NAME dicta-theme-qt COMMAND dicta-theme-qt-test)
    set_tests_properties(dicta-theme-qt PROPERTIES
        ENVIRONMENT "QT_QPA_PLATFORM=offscreen;QT_QPA_PLATFORMTHEME="
    )

    set(DICTA_CLI_BINARY "${DICTA_RUST_TARGET_DIR}/debug/dicta")
    file(GLOB_RECURSE DICTA_CLI_SOURCES CONFIGURE_DEPENDS
        "${CMAKE_CURRENT_SOURCE_DIR}/../dicta-cli/src/*.rs"
    )
    add_custom_command(
        OUTPUT "${DICTA_CLI_BINARY}"
        COMMAND cargo build
                --manifest-path "${DICTA_WORKSPACE_MANIFEST}"
                --target-dir "${DICTA_RUST_TARGET_DIR}"
                --package dicta-cli
                --locked
        DEPENDS
            ${DICTA_RUST_MANIFESTS}
            ${DICTA_RUST_SOURCES}
            ${DICTA_CLI_SOURCES}
            "${CMAKE_CURRENT_SOURCE_DIR}/../dicta-cli/Cargo.toml"
            "${DICTA_RUST_LOCKFILE}"
        COMMENT "Building the Dicta CLI for native process E2E"
        VERBATIM
    )
    add_custom_target(dicta_cli_test_build ALL DEPENDS "${DICTA_CLI_BINARY}")

    add_test(
        NAME dicta-native-process-e2e
        COMMAND bash
                "${CMAKE_CURRENT_SOURCE_DIR}/tests/native_process_e2e.sh"
                "$<TARGET_FILE:dicta-native>"
                "${DICTA_CLI_BINARY}"
    )
    set_tests_properties(dicta-native-process-e2e PROPERTIES
        TIMEOUT 20
    )
endif()
