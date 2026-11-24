import init, { start_dare } from "./pkg/oss_client.js";
import { connectZone } from "./transport.mjs";

const WASM = "pkg/oss_client_bg.wasm";
const button = document.querySelector("#start");
const status = document.querySelector("#status");
const entry = document.querySelector("#entry");
const progress = document.querySelector("#progress");
const bar = progress.querySelector("span");

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

function say(text, isError = false) {
  status.textContent = text;
  status.classList.toggle("error", isError);
}

function megabytes(bytes) {
  return (bytes / 1e6).toFixed(1);
}

// What stops this browser from running the game, or null if nothing does.
async function missingFeature() {
  if (typeof WebTransport === "undefined" || !navigator.gpu) {
    return "This browser can't run the game yet. Use desktop Chrome or Edge.";
  }
  const adapter = await navigator.gpu.requestAdapter().catch(() => null);
  if (!adapter) {
    return "Your browser has graphics acceleration (WebGPU) turned off or unavailable. Turn it on, or try desktop Chrome or Edge.";
  }
  return null;
}

// The uncompressed size of the game, written by build-client.sh. The server may
// compress the download, so the response's own length can't measure progress.
async function expectedSize() {
  try {
    const response = await fetch("build.json", { cache: "no-cache" });
    if (response.ok) return (await response.json()).wasmBytes ?? 0;
  } catch {}
  return 0;
}

// Downloads the game, reporting progress, and returns its bytes.
async function download() {
  const [total, response] = await Promise.all([expectedSize(), fetch(WASM)]);
  if (!response.ok || !response.body) throw new Error(`The game download failed (${response.status}).`);
  const reader = response.body.getReader();
  const chunks = [];
  let received = 0;
  for (;;) {
    const { value, done } = await reader.read();
    if (done) break;
    chunks.push(value);
    received += value.length;
    if (total > 0) {
      const percent = Math.min(99, Math.floor((received / total) * 100));
      bar.style.width = `${percent}%`;
      progress.setAttribute("aria-valuenow", String(percent));
      say(`Downloading the game… ${megabytes(received)} of ${megabytes(total)} MB`);
    } else {
      say(`Downloading the game… ${megabytes(received)} MB`);
    }
  }
  const bytes = new Uint8Array(received);
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.length;
  }
  return bytes;
}

function enter() {
  try {
    entry.hidden = true;
    const observer = new MutationObserver(() => {
      const canvas = document.querySelector("canvas");
      if (canvas) {
        canvas.tabIndex = 0;
        canvas.focus();
        observer.disconnect();
      }
    });
    observer.observe(document.body, { childList: true, subtree: true });
    start_dare();
  } catch (error) {
    entry.hidden = false;
    say(error?.message ?? String(error), true);
  }
}

try {
  const missing = await missingFeature();
  if (missing) throw new Error(missing);
  const bytes = await download();
  say("Starting…");
  await init({ module_or_path: bytes });
  progress.hidden = true;
  say("Ready.");
  button.disabled = false;
  button.focus();
  button.addEventListener("click", enter, { once: true });
} catch (error) {
  progress.hidden = true;
  say(error?.message ?? String(error), true);
}
