// The SOURCE Vault FFI catalogue (docs/security/phase-f2b-ffi.md §1–§2):
// the only engine entry points this app may call.
#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef struct Ov0Engine Ov0Engine;

typedef struct {
    void *ctx;
    int32_t (*secure_entry)(void *ctx, uint8_t kind, uint64_t timeout_ms, uint8_t *a, size_t *a_len, uint8_t *b, size_t *b_len, size_t cap);
    int32_t (*recovery_sheet)(void *ctx, const uint8_t *words, size_t words_len, const char *checkpoint, const char *recovery, const char *reason, uint64_t timeout_ms);
    bool (*presence)(void *ctx, const char *reason);
    bool (*capture_suppressed)(void *ctx, const char *surface);
    void (*event)(void *ctx, const uint8_t *json, size_t len);
} Ov0Callbacks;

const Ov0Engine *ov0_engine_open(const char *vault_dir, const Ov0Callbacks *callbacks);
int32_t ov0_engine_call(const Ov0Engine *engine, const uint8_t *request, size_t len, uint8_t **out, size_t *out_len);
void ov0_engine_lock(const Ov0Engine *engine);
void ov0_engine_free(uint8_t *buf);
void ov0_engine_close(const Ov0Engine *engine);
