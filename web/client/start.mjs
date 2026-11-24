import init, { start_dare } from "./pkg/oss_client.js";
import { connectZone } from "./transport.mjs";

const button = document.querySelector("#start");
const status = document.querySelector("#status");
const launch = document.querySelector("#launch");

globalThis.openDareGateway = () => {
  const queue = [];
  let link;
  let closed = false;
  const bridge = {
    serverIp: 0,
    boundPort: 0,
    error: undefined,
    receive: () => queue.shift(),
    send: (packet) => link?.send(packet),
    close() {
      closed = true;
      queue.length = 0;
      link?.close();
    },
  };
  function failed(error) {
    if (!closed) bridge.error = error?.message ?? String(error ?? "Gateway disconnected.");
  }
  void connectZone(
    "https://subspacefoundation.com:4443/play/transport",
    (packet) => {
      if (!closed && queue.length < 256) queue.push(packet);
    },
    failed,
  ).then((connected) => {
    if (closed) {
      connected.close();
      return;
    }
    link = connected;
    bridge.serverIp = link.serverIp;
    bridge.boundPort = link.boundPort;
  }).catch(failed);
  return bridge;
};

try {
  if (!navigator.gpu) throw new Error("Dare needs a browser with WebGPU.");
  await init();
  status.textContent = "";
  button.disabled = false;
  button.addEventListener("click", () => {
    try {
      launch.hidden = true;
      const observer = new MutationObserver(() => {
        const canvas = document.querySelector("canvas");
        if (canvas) {
          canvas.focus();
          observer.disconnect();
        }
      });
      observer.observe(document.body, { childList: true, subtree: true });
      start_dare();
    } catch (error) {
      launch.hidden = false;
      status.textContent = error?.message ?? String(error);
    }
  }, { once: true });
} catch (error) {
  status.textContent = error?.message ?? String(error);
}
