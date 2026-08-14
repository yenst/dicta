#ifndef DICTA_NATIVE_H
#define DICTA_NATIVE_H

#include <stdbool.h>

typedef void (*RecorderCallback)(const char *event, const char *message);

void dicta_start(const char *output_path, RecorderCallback callback);
void dicta_stop(RecorderCallback callback);
void dicta_transcribe(const char *input_path, const char *language, RecorderCallback callback);
bool dicta_extract_audio(const char *input_path, const char *output_path);
bool dicta_extract_poster(const char *input_path, const char *output_path);

#endif
