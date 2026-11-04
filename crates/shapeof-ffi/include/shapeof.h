#ifndef SHAPEOF_H
#define SHAPEOF_H

#include <stddef.h>
#include <stdint.h>

#define SHAPEOF_OK 0

#define SHAPEOF_INVALID_UTF8 1

#define SHAPEOF_INVALID_ARGUMENT 2

#define SHAPEOF_INTERNAL 3

#define SHAPEOF_OPENAI 0

#define SHAPEOF_ANTHROPIC 1

#define SHAPEOF_PLAIN 2

typedef struct shapeof_report {
  uint32_t status;
  uint8_t *data;
  size_t len;
} shapeof_report;

#ifdef __cplusplus
extern "C" {
#endif // __cplusplus

void shapeof_infer(const uint8_t *input,
                   size_t input_len,
                   uint32_t target,
                   struct shapeof_report *out);

void shapeof_report_free(struct shapeof_report *report);

const char *shapeof_version(void);

uint8_t *shapeof_alloc(size_t len);

void shapeof_dealloc(uint8_t *p, size_t len);

#ifdef __cplusplus
}  // extern "C"
#endif  // __cplusplus

#endif  /* SHAPEOF_H */
