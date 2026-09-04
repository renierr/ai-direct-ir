#ifndef WASI_SHIM_H
#define WASI_SHIM_H

#include "wasm-rt.h"

#include <stdbool.h>
#include <stdint.h>

#ifndef WASM_RT_CORE_TYPES_DEFINED
#define WASM_RT_CORE_TYPES_DEFINED
typedef uint8_t u8;
typedef int8_t s8;
typedef uint16_t u16;
typedef int16_t s16;
typedef uint32_t u32;
typedef int32_t s32;
typedef uint64_t u64;
typedef int64_t s64;
typedef float f32;
typedef double f64;
#endif

/* Definition of the opaque host struct the wasm2c-generated headers refer to.
 * A single shared instance is passed to every instantiated module. */
struct w2c_wasi__snapshot__preview1 {
  int unused;
};

/* Linear memory of the currently running module (single instance at a time).
 * Set by main() right after instantiate, read by the fd_* shims. */
void wasi_shim_set_memory(wasm_rt_memory_t *mem);

#endif /* WASI_SHIM_H */
