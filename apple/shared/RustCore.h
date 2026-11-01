#ifndef MOMENTSTAYS_RUST_CORE_H
#define MOMENTSTAYS_RUST_CORE_H
#include <stddef.h>
#include <stdint.h>
// Borrowed input; opaque owned response. Output pointers expire at free.
void *ms_core_call(uint32_t operation, const uint8_t *input, size_t len);
const uint8_t *ms_core_bytes(const void *response, uint32_t slot, size_t *len);
void ms_core_free(void *response);

// Batch engine (core/ENGINE.md). Request: the driver JSON; reply: slot 0 JSON.
// Codec callbacks may run concurrently on Rust worker threads; any may be NULL.
typedef struct MsBuffer MsBuffer;
void ms_buffer_append(MsBuffer *buffer, const uint8_t *data, size_t len);
typedef struct {
    void *context;
    int32_t (*image_decodes)(void *, const uint8_t *, size_t);
    int32_t (*srgb_icc)(void *, MsBuffer *out, MsBuffer *err);
    int32_t (*decode_gray_jpeg)(void *, const uint8_t *, size_t, MsBuffer *pixels, int64_t *dims3, MsBuffer *err);
    int32_t (*encode_gray_jpeg)(void *, const uint8_t *, size_t, int64_t w, int64_t h, int64_t stride, double quality, MsBuffer *out, MsBuffer *err);
    int32_t (*reencode_jpeg)(void *, const char *path, double quality, MsBuffer *out, MsBuffer *err);
    int32_t (*heic_gain_map)(void *, const char *path, MsBuffer *info_json, MsBuffer *pixels, MsBuffer *err);
    int32_t (*frame_jpeg)(void *, const char *video, int64_t track, double seconds, MsBuffer *out, int64_t *dims2, MsBuffer *err);
    void (*progress)(void *, const uint8_t *json, size_t len);
    int32_t (*existing_output)(void *, const char *source, const char *target);
    void (*rows_ready)(void *, const uint8_t *json, size_t len);
} MsNative;
void *ms_stop_new(void);
void ms_stop_cancel(const void *token);
void ms_stop_free(void *token);
void *ms_core_engine(const MsNative *native, const uint8_t *request, size_t len, const void *token);
#endif
