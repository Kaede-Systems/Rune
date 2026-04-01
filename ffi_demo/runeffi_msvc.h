#ifndef RUNE_RUNEFFI_MSVC_H
#define RUNE_RUNEFFI_MSVC_H

#include <stdbool.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

int32_t add(int32_t a, int32_t b);
int32_t mul(int32_t a, int32_t b);

#ifdef __cplusplus
}
#endif

#endif /* RUNE_RUNEFFI_MSVC_H */
