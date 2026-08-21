#ifndef DICTA_NATIVE_H
#define DICTA_NATIVE_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

enum {
    DICTA_NATIVE_HOST_FLAG_E2E = 1
};

enum {
    DICTA_NATIVE_OVERLAY_SHOW = 1,
    DICTA_NATIVE_OVERLAY_START_CLOCK = 2,
    DICTA_NATIVE_OVERLAY_SET_ENABLED = 3,
    DICTA_NATIVE_OVERLAY_SET_TOOL = 4,
    DICTA_NATIVE_OVERLAY_UNDO = 5,
    DICTA_NATIVE_OVERLAY_CLEAR = 6,
    DICTA_NATIVE_OVERLAY_FINISH = 7,
    DICTA_NATIVE_UI_SHOW_REQUESTED = 8,
    DICTA_NATIVE_UI_OPEN_RECORDING_REQUESTED = 9
};

enum {
    DICTA_NATIVE_ANNOTATION_ENABLE = 1,
    DICTA_NATIVE_ANNOTATION_DISABLE = 2,
    DICTA_NATIVE_ANNOTATION_SET_TOOL = 3,
    DICTA_NATIVE_ANNOTATION_UNDO = 4,
    DICTA_NATIVE_ANNOTATION_CLEAR = 5
};

enum {
    DICTA_NATIVE_TOOL_PEN = 0,
    DICTA_NATIVE_TOOL_ARROW = 1,
    DICTA_NATIVE_TOOL_RECTANGLE = 2,
    DICTA_NATIVE_TOOL_SPOTLIGHT = 3
};

enum {
    DICTA_NATIVE_SETTINGS_SHORTCUT = 1,
    DICTA_NATIVE_SETTINGS_CLEANUP = 2,
    DICTA_NATIVE_SETTINGS_BRANCH_LOCKING = 3,
    DICTA_NATIVE_SETTINGS_LANGUAGE = 4,
    DICTA_NATIVE_SETTINGS_GENERAL_PATH = 5
};

enum {
    DICTA_NATIVE_CODEX_MCP_CONNECT = 1,
    DICTA_NATIVE_CODEX_MCP_RESTART = 2
};

enum {
    DICTA_NATIVE_UI_SNAPSHOT_MAX_BYTES = 64 * 1024,
    DICTA_NATIVE_RECORDING_LIST_MAX_BYTES = 64 * 1024,
    DICTA_NATIVE_RECORDING_DETAIL_MAX_BYTES = 1024 * 1024,
    DICTA_NATIVE_CLEANUP_SUMMARY_MAX_BYTES = 64 * 1024,
    DICTA_NATIVE_CODEX_MCP_STATUS_MAX_BYTES = 16 * 1024,
    DICTA_NATIVE_VOICE_NOTE_STATUS_MAX_BYTES = 16 * 1024
};

struct DictaNativeHostConfig {
    const unsigned char *socket_path;
    size_t socket_path_len;
    const unsigned char *storage_root;
    size_t storage_root_len;
    const unsigned char *output_name;
    size_t output_name_len;
    uint32_t flags;
};

struct DictaNativeOverlayCommand {
    uint32_t kind;
    uint32_t tool;
    const unsigned char *output_name;
    size_t output_name_len;
};

typedef void (*DictaNativeOverlayCallback)(void *context, const struct DictaNativeOverlayCommand *command);

const char *dicta_native_api_version(void);
int dicta_native_host_start(
    const struct DictaNativeHostConfig *config,
    DictaNativeOverlayCallback callback,
    void *callback_context
);
void dicta_native_host_request_stop(void);
int dicta_native_host_join(void);
uint32_t dicta_native_host_state(void);
uint64_t dicta_native_host_stroke_count(void);
size_t dicta_native_host_last_error(unsigned char *output, size_t capacity);
int dicta_native_host_overlay_stroke(
    uint32_t tool,
    double started_at_seconds,
    double ended_at_seconds,
    const double *xy,
    size_t point_count
);
int dicta_native_host_record_start(const unsigned char *note, size_t note_length);
int dicta_native_host_record_stop(void);
int dicta_native_host_annotation_command(uint32_t action, uint32_t tool);
int dicta_native_host_settings_set(uint32_t key, const unsigned char *value, size_t value_length);
size_t dicta_native_host_cleanup_merged(
    const unsigned char *project_id,
    size_t project_id_length,
    unsigned char *output,
    size_t capacity
);
int dicta_native_host_model_install_quality(void);
size_t dicta_native_codex_mcp_status(unsigned char *output, size_t capacity);
size_t dicta_native_codex_mcp_action(uint32_t action, unsigned char *output, size_t capacity);
int dicta_native_voice_note_start(
    const unsigned char *recording_id,
    size_t recording_id_length,
    double timestamp_seconds
);
int dicta_native_voice_note_stop(void);
int dicta_native_voice_note_cancel(void);
size_t dicta_native_voice_note_status(unsigned char *output, size_t capacity);
size_t dicta_native_host_ui_snapshot(unsigned char *output, size_t capacity);
size_t dicta_native_host_recordings_for_project(
    const unsigned char *project_id,
    size_t project_id_length,
    unsigned char *output,
    size_t capacity
);
size_t dicta_native_host_recording_detail(
    const unsigned char *recording_id,
    size_t recording_id_length,
    unsigned char *output,
    size_t capacity
);
int dicta_native_host_recording_delete(const unsigned char *recording_id, size_t recording_id_length);
int dicta_native_host_recording_transcribe(
    const unsigned char *recording_id,
    size_t recording_id_length
);
int dicta_native_host_timeline_notes_set(
    const unsigned char *recording_id,
    size_t recording_id_length,
    const unsigned char *notes_json,
    size_t notes_json_length
);
int dicta_native_host_project_select(const unsigned char *project_id, size_t project_id_length);
int dicta_native_host_project_remove(const unsigned char *project_id, size_t project_id_length);
int dicta_native_host_project_add(const unsigned char *path, size_t path_length);
int dicta_native_host_project_create(const unsigned char *name, size_t name_length);
size_t dicta_native_host_recording_context(
    const unsigned char *recording_id,
    size_t recording_id_length,
    const unsigned char *project_id,
    size_t project_id_length,
    unsigned char *output,
    size_t capacity
);

#ifdef __cplusplus
}
#endif

#endif
