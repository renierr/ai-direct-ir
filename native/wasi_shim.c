/* Minimal WASI preview1 shim for wasm2c-built modules.
 *
 * Implements exactly the 3 imports our modules use: fd_write, fd_read,
 * proc_exit — on top of POSIX read/write/exit. This file (+ the C compiler)
 * is the new trusted computing base that replaces the wasmtime sandbox,
 * so every (offset,len) pair coming from the module is validated against
 * the actual linear-memory size with overflow-safe arithmetic. A bug in the
 * WASM module itself can only ever produce a TRAP or an error return, never
 * a host-side OOB — *if* this validation stays correct. Keep it simple.
 *
 * WASI errno values match wasi-sdk (__WASI_ERRNO_*).
 */
#define _POSIX_C_SOURCE 200809L
#include "wasi_shim.h"

#include <errno.h>
#include <stdio.h>
#include <stdlib.h>

#define WASI_ESUCCESS 0
#define WASI_EBADF 8
#define WASI_EFAULT 21
#define WASI_EINVAL 28
#define WASI_EIO 29

static wasm_rt_memory_t *g_mem = NULL;

void wasi_shim_set_memory(wasm_rt_memory_t *mem) {
  g_mem = mem;
}

/* true iff [off, off+len) lies inside linear memory (overflow-safe). */
static bool range_ok(u32 off, u32 len) {
  if (g_mem == NULL) {
    return false;
  }
  return (u64)off + (u64)len <= (u64)g_mem->size;
}

static bool read_u32(u32 off, u32 *out) {
  if (!range_ok(off, 4)) {
    return false;
  }
  /* WASM memory is little-endian; x86_64/ARM64 hosts too. */
  __builtin_memcpy(out, &g_mem->data[off], 4);
  return true;
}

/* import: 'wasi_snapshot_preview1' 'fd_write' */
u32 w2c_wasi__snapshot__preview1_fd_write(struct w2c_wasi__snapshot__preview1 *inst,
                                          u32 fd, u32 iovs, u32 iovs_len,
                                          u32 nwritten_ptr) {
  (void)inst;
  FILE *stream;
  u32 total = 0;

  if (fd != 1 && fd != 2) {
    return WASI_EBADF;
  }
  stream = (fd == 1) ? stdout : stderr;
  if (!range_ok(nwritten_ptr, 4)) {
    return WASI_EFAULT;
  }
  for (u32 i = 0; i < iovs_len; i++) {
    u32 ptr, len, vec;
    if (!range_ok(iovs + i * 8, 8)) {
      return WASI_EFAULT;
    }
    if (!read_u32(iovs + i * 8, &ptr) || !read_u32(iovs + i * 8 + 4, &len)) {
      return WASI_EFAULT;
    }
    if (!range_ok(ptr, len)) {
      return WASI_EFAULT;
    }
    vec = 0;
    while (vec < len) {
      size_t n = fwrite(&g_mem->data[ptr + vec], 1, (size_t)(len - vec), stream);
      vec += (u32)n;
      if (n == 0) {
        if (ferror(stream)) {
          return WASI_EIO;
        }
        break;
      }
    }
    total += vec;
    if (vec != len) {
      break; /* short write: report what went out */
    }
  }
  /* Unbuffered semantics like POSIX write: prompt must be visible
   * before the next fd_read blocks (matters on pipes + Windows). */
  fflush(stream);
  __builtin_memcpy(&g_mem->data[nwritten_ptr], &total, 4);
  return WASI_ESUCCESS;
}

/* import: 'wasi_snapshot_preview1' 'fd_read' */
u32 w2c_wasi__snapshot__preview1_fd_read(struct w2c_wasi__snapshot__preview1 *inst,
                                         u32 fd, u32 iovs, u32 iovs_len,
                                         u32 nread_ptr) {
  (void)inst;
  u32 total = 0;

  if (fd != 0) {
    return WASI_EBADF;
  }
  if (!range_ok(nread_ptr, 4)) {
    return WASI_EFAULT;
  }
  for (u32 i = 0; i < iovs_len; i++) {
    u32 ptr, len;
    if (!range_ok(iovs + i * 8, 8)) {
      return WASI_EFAULT;
    }
    if (!read_u32(iovs + i * 8, &ptr) || !read_u32(iovs + i * 8 + 4, &len)) {
      return WASI_EFAULT;
    }
    if (!range_ok(ptr, len)) {
      return WASI_EFAULT;
    }
    /* Single read per iovec: correct for pipes/TTYs, keeps semantics obvious. */
    size_t n = fread(&g_mem->data[ptr], 1, (size_t)len, stdin);
    if (n == 0 && ferror(stdin)) {
      return WASI_EIO;
    }
    total += (u32)n;
    break; /* one read total, like a TTY/pipe short read */
  }
  (void)iovs_len;
  __builtin_memcpy(&g_mem->data[nread_ptr], &total, 4);
  return WASI_ESUCCESS;
}

/* import: 'wasi_snapshot_preview1' 'proc_exit' */
void w2c_wasi__snapshot__preview1_proc_exit(struct w2c_wasi__snapshot__preview1 *inst,
                                            u32 code) {
  (void)inst;
  exit((int)code);
}
