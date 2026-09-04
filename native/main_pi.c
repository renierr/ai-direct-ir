/* Native host for pi.wasm (wasm2c build): instantiate + call _start. */
#include <stdio.h>

#include "pi.h"
#include "wasi_shim.h"

static struct w2c_wasi__snapshot__preview1 g_wasi;

int main(void) {
  w2c_pi mod;
  wasm_rt_init();
  wasm2c_pi_instantiate(&mod, &g_wasi);
  wasi_shim_set_memory(&mod.w2c_memory);
  w2c_pi_0x5Fstart(&mod);
  wasm2c_pi_free(&mod);
  wasm_rt_free();
  return 0;
}
