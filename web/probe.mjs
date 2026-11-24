import init, { BrowserSession } from "./pkg/dare_web_session.js";
import { connectZone } from "./transport.mjs";

const form = document.querySelector("#login");
const disconnect = document.querySelector("#disconnect");
const status = document.querySelector("#status");
const submit = form.querySelector("button");
const canvas = document.querySelector("canvas");
const endpoint = "https://subspacefoundation.com:4443/play/transport";
let active = null;

async function makePlot() {
  const adapter = await navigator.gpu?.requestAdapter();
  if (!adapter) throw new Error("WebGPU is unavailable.");
  const device = await adapter.requestDevice();
  const context = canvas.getContext("webgpu");
  const format = navigator.gpu.getPreferredCanvasFormat();
  context.configure({ device, format });
  const module = device.createShaderModule({ code: `
    @vertex fn vertex(@location(0) pixel: vec2f,
                       @builtin(vertex_index) corner: u32) -> @builtin(position) vec4f {
      let offsets = array<vec2f, 6>(vec2f(-1,-1), vec2f(1,-1), vec2f(1,1),
                                    vec2f(-1,-1), vec2f(1,1), vec2f(-1,1));
      let center = pixel / 8192.0 * vec2f(1,-1) + vec2f(-1,1);
      return vec4f(center + offsets[corner] * 0.007, 0, 1);
    }
    @fragment fn fragment() -> @location(0) vec4f { return vec4f(0.39,0.89,1.0,1.0); }
  ` });
  const pipeline = await device.createRenderPipelineAsync({
    layout: "auto",
    vertex: { module, entryPoint: "vertex", buffers: [
      { arrayStride: 8, stepMode: "instance", attributes: [{ shaderLocation: 0, offset: 0, format: "float32x2" }] },
    ] },
    fragment: { module, entryPoint: "fragment", targets: [{ format }] },
  });
  const maxPilots = 4096;
  const buffer = device.createBuffer({ size: maxPilots * 8, usage: GPUBufferUsage.VERTEX | GPUBufferUsage.COPY_DST });
  return (positions) => {
    const visible = positions.subarray(0, maxPilots * 2);
    if (visible.length) device.queue.writeBuffer(buffer, 0, visible);
    const encoder = device.createCommandEncoder();
    const pass = encoder.beginRenderPass({ colorAttachments: [{
      view: context.getCurrentTexture().createView(),
      loadOp: "clear", storeOp: "store", clearValue: { r: 0.012, g: 0.027, b: 0.06, a: 1 },
    }] });
    pass.setPipeline(pipeline);
    pass.setVertexBuffer(0, buffer);
    pass.draw(6, visible.length / 2);
    pass.end();
    device.queue.submit([encoder.finish()]);
  };
}

function stop(message) {
  const current = active;
  active = null;
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
      current.timer = setInterval(() => {
        const error = session.update(now());
        if (error) {
          stop(error);
          return;
        }
        for (let packet = session.transmit(); packet; packet = session.transmit()) link.send(packet);
        const state = session.state();
        status.textContent = state;
        if (state === "Closed") stop("The game session closed.");
      }, 10);
      function frame() {
        if (active !== current) return;
        draw(session.positions());
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
