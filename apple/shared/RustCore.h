#ifndef MOMENTSTAYS_RUST_CORE_H
#define MOMENTSTAYS_RUST_CORE_H
#include <stddef.h>
#include <stdint.h>
// Borrowed input; opaque owned response. Output pointers expire at free.
void *ms_core_call(uint32_t operation, const uint8_t *input, size_t len);
const uint8_t *ms_core_bytes(const void *response, uint32_t slot, size_t *len);
void ms_core_free(void *response);
#endif
