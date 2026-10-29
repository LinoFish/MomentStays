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
void momentstays_free_string(char *s);
void momentstays_free_bytes(uint8_t *ptr, uintptr_t len);
#endif
