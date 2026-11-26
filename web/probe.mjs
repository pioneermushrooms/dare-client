import init, { BrowserSession } from "./pkg/dare_web_session.js";
import { connectZone } from "./transport.mjs";

const form = document.querySelector("#login");
const disconnect = document.querySelector("#disconnect");
const status = document.querySelector("#status");
const submit = form.querySelector("button");
const canvas = document.querySelector("canvas");
const endpoint = "https://subspacefoundation.com:4443/play/transport";
let active = null;
const controls = { ArrowUp: 1, ArrowDown: 2, ArrowLeft: 4, ArrowRight: 8, KeyX: 16, KeyZ: 32 };
let keys = 0;
for (const event of ["keydown", "keyup"]) {
  window.addEventListener(event, (input) => {
    const control = controls[input.code];
    if (!active || !control) return;
    input.preventDefault();
    keys = event === "keydown" ? keys | control : keys & ~control;
  });
}
window.addEventListener("blur", () => { keys = 0; });

async function makePlot() {
  const adapter = await navigator.gpu?.requestAdapter();
  if (!adapter) throw new Error("WebGPU is unavailable.");
  const device = await adapter.requestDevice();
  const context = canvas.getContext("webgpu");
  const format = navigator.gpu.getPreferredCanvasFormat();
  context.configure({ device, format });
  const module = device.createShaderModule({ code: `
    struct Vertex { @builtin(position) position: vec4f, @location(0) color: vec3f };
    @vertex fn vertex(@location(0) pixel: vec4f,
                       @builtin(vertex_index) corner: u32) -> Vertex {
      let offsets = array<vec2f, 6>(vec2f(-1,-1), vec2f(1,-1), vec2f(1,1),
                                    vec2f(-1,-1), vec2f(1,1), vec2f(-1,1));
      let center = pixel.xy * vec2f(1,-1);
      let colors = array<vec3f, 4>(vec3f(0.25,0.35,0.46), vec3f(0.3,0.9,1),
                                   vec3f(0.95,0.55,0.35), vec3f(1,0.97,0.62));
      return Vertex(vec4f(center + offsets[corner] * pixel.z, 0, 1), colors[u32(pixel.w)]);
    }
    @fragment fn fragment(input: Vertex) -> @location(0) vec4f { return vec4f(input.color,1); }
  ` });
  const pipeline = await device.createRenderPipelineAsync({
    layout: "auto",
    vertex: { module, entryPoint: "vertex", buffers: [
      { arrayStride: 16, stepMode: "instance", attributes: [{ shaderLocation: 0, offset: 0, format: "float32x4" }] },
    ] },
    fragment: { module, entryPoint: "fragment", targets: [{ format }] },
  });
  const capacity = 16384;
  const instances = new Float32Array(capacity * 4);
  const buffer = device.createBuffer({ size: instances.byteLength, usage: GPUBufferUsage.VERTEX | GPUBufferUsage.COPY_DST });
  return (positions, tiles, shots) => {
    const midX = positions[0] ?? 8192;
    const midY = positions[1] ?? 8192;
    const radius = 512;
    let next = 0;
    function batch(points, size, color, limit) {
      for (let i = 0; i < points.length && next < limit * 4; i += 2) {
        const x = (points[i] - midX) / radius;
        const y = (points[i + 1] - midY) / radius;
        if (Math.abs(x) > 1.1 || Math.abs(y) > 1.1) continue;
        instances.set([x, y, size, color === 2 && i === 0 ? 1 : color], next);
        next += 4;
      }
    }
    batch(tiles, 8 / radius, 0, capacity - 2048);
    batch(positions, 10 / radius, 2, capacity - 1024);
    batch(shots, 3 / radius, 3, capacity);
    if (next) device.queue.writeBuffer(buffer, 0, instances.subarray(0, next));
    const encoder = device.createCommandEncoder();
    const pass = encoder.beginRenderPass({ colorAttachments: [{
      view: context.getCurrentTexture().createView(),
      loadOp: "clear", storeOp: "store", clearValue: { r: 0.012, g: 0.027, b: 0.06, a: 1 },
    }] });
    pass.setPipeline(pipeline);
    pass.setVertexBuffer(0, buffer);
    pass.draw(6, next / 4);
    pass.end();
    device.queue.submit([encoder.finish()]);
  };
}

function stop(message) {
  const current = active;
  active = null;
  keys = 0;
  if (current) {
    clearInterval(current.timer);
    cancelAnimationFrame(current.frame);
    current.link.close();
    current.session.free();
  }
  status.textContent = message;
  submit.disabled = false;
  disconnect.disabled = true;
}

submit.disabled = true;
try {
  await init();
  const draw = await makePlot();
  status.textContent = "Ready to connect.";
  submit.disabled = false;

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    submit.disabled = true;
    status.textContent = "Opening gateway connection…";
    const values = new FormData(form);
    form.elements.namedItem("password").value = "";
    const start = performance.now();
    const now = () => Math.floor(performance.now() - start);
    let link;
    try {
      link = await connectZone(endpoint, (bytes) => active?.session.receive(now(), bytes),
        (error) => stop(error?.message ?? "Gateway connection closed."));
      const seed = crypto.getRandomValues(new Uint32Array(1))[0];
      const session = new BrowserSession(values.get("pilot"), values.get("password"),
        values.get("arena").trim(), now(), seed, link.serverIp, link.boundPort);
      active = { link, session, now };
      const current = active;
      disconnect.disabled = false;
      let tiles = new Float32Array();
      current.timer = setInterval(() => {
        const error = session.update(now(), keys);
        if (error) {
          stop(error);
          return;
        }
        for (let packet = session.transmit(); packet; packet = session.transmit()) link.send(packet);
        const state = session.state();
        if (state === "Playing" && status.textContent !== state) tiles = session.tiles();
        status.textContent = state;
        if (state === "Closed") stop("The game session closed.");
      }, 10);
      function frame() {
        if (active !== current) return;
        draw(session.positions(), tiles, session.projectiles());
        current.frame = requestAnimationFrame(frame);
      }
      current.frame = requestAnimationFrame(frame);
    } catch (error) {
      link?.close();
      stop(error.message);
    }
  });
  disconnect.addEventListener("click", () => {
    if (!active) return;
    const { session, link, now } = active;
    session.disconnect(now());
    for (let packet = session.transmit(); packet; packet = session.transmit()) link.send(packet);
    stop("Disconnected.");
  });
} catch (error) {
  status.textContent = error.message;
}
