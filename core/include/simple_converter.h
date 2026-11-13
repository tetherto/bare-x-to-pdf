#ifndef SIMPLE_CONVERTER_H
#define SIMPLE_CONVERTER_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

enum {
  SC_STATUS_OK = 0,
  SC_STATUS_ERROR = 1,
  SC_STATUS_PANIC = 2,
  SC_STATUS_INVALID = 3,
  SC_STATUS_LIMIT = 4,
};

typedef struct {
  uint8_t *data;
  size_t len;
} sc_buffer_t;

int
sc_convert(
  const uint8_t *input,
  size_t input_len,
  const char *from,
  const char *to,
  const char *fonts_dir,
  sc_buffer_t *out,
  sc_buffer_t *error
);

void
sc_free(sc_buffer_t *buffer);

#ifdef __cplusplus
}
#endif

#endif
