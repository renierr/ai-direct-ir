const canvas = document.querySelector("#app");
const ctx = canvas.getContext("2d");
const keys = new Set();
let mouseX = 0;
let mouseY = 0;
let instance;

addEventListener("keydown", (event) => keys.add(event.code));
addEventListener("keyup", (event) => keys.delete(event.code));
canvas.addEventListener("pointermove", (event) => {
  const bounds = canvas.getBoundingClientRect();
  mouseX = Math.trunc((event.clientX - bounds.left) * canvas.width / bounds.width);
  mouseY = Math.trunc((event.clientY - bounds.top) * canvas.height / bounds.height);
});

const web = {
  canvas_width: () => canvas.width,
  canvas_height: () => canvas.height,
  clear: (r, g, b, a) => {
    ctx.fillStyle = `rgba(${r}, ${g}, ${b}, ${a / 255})`;
    ctx.fillRect(0, 0, canvas.width, canvas.height);
  },
  fill_rect: (x, y, width, height, r, g, b, a) => {
    ctx.fillStyle = `rgba(${r}, ${g}, ${b}, ${a / 255})`;
    ctx.fillRect(x, y, width, height);
  },
  request_frame: () => requestAnimationFrame(() => instance.exports.frame()),
  key_down: (key) => keys.has(keyName(key)) ? 1 : 0,
  mouse_x: () => mouseX,
  mouse_y: () => mouseY,
};

function keyName(key) {
  return ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Space"][key] ?? "";
}

({ instance } = await WebAssembly.instantiateStreaming(fetch("__APPNAME__.wasm"), { web }));
instance.exports.start();
