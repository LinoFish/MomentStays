#ifndef MOMENTSTAYS_RUST_CORE_H
#define MOMENTSTAYS_RUST_CORE_H
#include <stddef.h>
#include <stdint.h>
// New C ABI (core/include/momentstays_core.h). Borrowed input; owned
// outputs freed by the matching free function. JSON outputs are
// NUL-terminated strings freed by momentstays_free_string. Still bytes
// from momentstays_parse are freed by momentstays_free_bytes.
const char *momentstays_version(void);
int32_t momentstays_inspect(const uint8_t *data, uintptr_t len, char **out_json);
int32_t momentstays_parse(const uint8_t *data, uintptr_t len, char **out_json,
                          const uint8_t **out_still, uintptr_t *out_still_len);
int32_t momentstays_coding_hash(const uint8_t *data, uintptr_t len, char **out_json);
int32_t momentstays_auxiliary_hashes(const uint8_t *data, uintptr_t len, char **out_json);
// Conversion entries (see core/include/momentstays_core.h for the
// contract): JSON `status` is "ok", "needs_pixel_work", or "error".
int32_t momentstays_to_live(const uint8_t *motion, uintptr_t motion_len, const char *opts_json,
                            const uint8_t *still_override, uintptr_t still_override_len, int32_t has_override,
                            char **out_json, const uint8_t **out_still, uintptr_t *out_still_len,
                            const uint8_t **out_mov, uintptr_t *out_mov_len);
int32_t momentstays_to_motion(const uint8_t *still, uintptr_t still_len, const uint8_t *movie, uintptr_t movie_len,
                              const char *opts_json, char **out_json,
                              const uint8_t **out_bytes, uintptr_t *out_bytes_len);
int32_t momentstays_attach_gain(const uint8_t *base, uintptr_t base_len, const uint8_t *gain, uintptr_t gain_len,
                                double stops, const uint8_t *srgb_icc, uintptr_t srgb_icc_len, int32_t has_icc,
                                char **out_json, const uint8_t **out_bytes, uintptr_t *out_bytes_len);
int32_t momentstays_live_still(const uint8_t *still, uintptr_t still_len, const char *uuid, int32_t update_identifier,
                               char **out_json, const uint8_t **out_bytes, uintptr_t *out_bytes_len);
int32_t momentstays_scan_row(const uint8_t *data, uintptr_t len, char **out_json);
int32_t momentstays_cover_map(const uint8_t *video, uintptr_t video_len, const char *opts_json, char **out_json);
int32_t momentstays_export_auxiliary(const uint8_t *motion, uintptr_t motion_len, char **out_json,
                                     const uint8_t **out_mov, uintptr_t *out_mov_len);
int32_t momentstays_vendor_convert(const uint8_t *motion, uintptr_t motion_len, const char *target,
                                   char **out_json, const uint8_t **out_bytes, uintptr_t *out_bytes_len);
int32_t momentstays_candidates(const uint8_t *video, uintptr_t video_len, char **out_json);
int32_t momentstays_live_pair(const uint8_t *still, uintptr_t still_len, const uint8_t *movie, uintptr_t movie_len,
                              const char *time_domain, char **out_json);
void momentstays_free_string(char *s);
void momentstays_free_bytes(uint8_t *ptr, uintptr_t len);
#endif
