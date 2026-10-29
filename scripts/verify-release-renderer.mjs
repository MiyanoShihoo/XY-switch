import { mkdir, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";

const endpoint = process.env.CDP_ENDPOINT ?? "http://127.0.0.1:9224";
const expectedUrl = process.env.APP_URL ?? "http://tauri.localhost/";
const output = resolve(process.env.SCREENSHOT_PATH ?? "artifacts/release-startup.png");
const verifyProxyToggle = process.env.VERIFY_PROXY_TOGGLE === "1";

const pages = await fetch(`${endpoint}/json`).then((response) => {
  if (!response.ok) throw new Error(`CDP endpoint returned ${response.status}`);
  return response.json();
});
const page = pages.find((candidate) => candidate.type === "page" && candidate.url === expectedUrl);
if (!page) {
  throw new Error(`Release page not found at ${expectedUrl}: ${JSON.stringify(pages)}`);
}
if (page.url.includes("localhost:1420")) {
  throw new Error(`Release is still loading the Vite development server: ${page.url}`);
}

const socket = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((resolveOpen, rejectOpen) => {
  socket.addEventListener("open", resolveOpen, { once: true });
  socket.addEventListener("error", rejectOpen, { once: true });
});

let sequence = 0;
const pending = new Map();
socket.addEventListener("message", async (event) => {
  const raw = typeof event.data === "string"
    ? event.data
    : event.data instanceof Blob
      ? await event.data.text()
      : Buffer.from(event.data).toString("utf8");
  const message = JSON.parse(raw);
  if (!message.id || !pending.has(message.id)) return;
  const request = pending.get(message.id);
  pending.delete(message.id);
  if (message.error) request.reject(new Error(message.error.message));
  else request.resolve(message.result);
});

function call(method, params = {}) {
  const id = ++sequence;
  return new Promise((resolveCall, rejectCall) => {
    const timeout = setTimeout(() => {
      pending.delete(id);
      rejectCall(new Error(`CDP call timed out: ${method}`));
    }, 5000);
    pending.set(id, {
      resolve: (value) => {
        clearTimeout(timeout);
        resolveCall(value);
      },
      reject: (error) => {
        clearTimeout(timeout);
        rejectCall(error);
      },
    });
    socket.send(JSON.stringify({ id, method, params }));
  });
}

async function evaluate(expression) {
  const result = await call("Runtime.evaluate", {
    expression,
    awaitPromise: true,
    returnByValue: true,
  });
  if (result.exceptionDetails) {
    throw new Error(result.exceptionDetails.exception?.description ?? result.exceptionDetails.text);
  }
  return result.result.value;
}

await call("Emulation.setDeviceMetricsOverride", {
  width: 1180,
  height: 760,
  deviceScaleFactor: 1,
  mobile: false,
});
const state = await evaluate(`(async () => {
  const proxy = await window.__TAURI_INTERNALS__.invoke('get_proxy_status');
  return {
    readyState: document.readyState,
    title: document.title,
    bodyText: document.body.innerText,
    rootChildren: document.querySelector('#root')?.childElementCount ?? 0,
    navigation: [...document.querySelectorAll(".xy-shell-nav-item")].map(node => node.title),
    runtimeVisible: Boolean(document.querySelector(".xy-shell-runtime")),
    horizontalOverflow: document.documentElement.scrollWidth > innerWidth,
    proxy,
  };
})()`);

if (state.readyState !== "complete" || state.rootChildren === 0) {
  throw new Error(`Release renderer did not finish loading: ${JSON.stringify(state)}`);
}
if (!state.bodyText.includes("供应商") || !state.bodyText.includes("XY Switch")) {
  throw new Error(`Release renderer is missing primary UI content: ${JSON.stringify(state)}`);
}
if (state.navigation.length !== 7 || !state.runtimeVisible) {
  throw new Error(`Release workbench shell is incomplete: ${JSON.stringify(state)}`);
}
if (state.horizontalOverflow) {
  throw new Error("Release renderer overflows horizontally at 1180x760");
}
if (state.proxy.running || state.proxy.settings.enabled) {
  throw new Error(`Local proxy started automatically: ${JSON.stringify(state.proxy)}`);
}

let manualProxyToggle;
if (verifyProxyToggle) {
  if (process.env.QA_ISOLATED !== "1") throw new Error("Proxy mutation requires QA_ISOLATED=1 and an isolated native test profile");
  let started;
  let health;
  try {
    started = await evaluate(
      "window.__TAURI_INTERNALS__.invoke('set_proxy_enabled', { enabled: true })",
    );
    if (!started.running || !started.settings.enabled) {
      throw new Error(`Manual proxy start failed: ${JSON.stringify(started)}`);
    }
    const response = await fetch(`${started.address}/health`);
    if (!response.ok) throw new Error(`Proxy health endpoint returned ${response.status}`);
    health = await response.json();
  } finally {
    await evaluate(
      "window.__TAURI_INTERNALS__.invoke('set_proxy_enabled', { enabled: false })",
    );
  }
  const stopped = await evaluate(
    "window.__TAURI_INTERNALS__.invoke('get_proxy_status')",
  );
  if (stopped.running || stopped.settings.enabled) {
    throw new Error(`Manual proxy stop failed: ${JSON.stringify(stopped)}`);
  }
  let acceptsConnections = false;
  try {
    await fetch(`${stopped.address}/health`, { signal: AbortSignal.timeout(500) });
    acceptsConnections = true;
  } catch {
    // A refused connection is the expected result after an explicit stop.
  }
  if (acceptsConnections) throw new Error("Proxy still accepts connections after manual stop");
  manualProxyToggle = { started, health, stopped };
}

const screenshot = await call("Page.captureScreenshot", { format: "png", fromSurface: true });
await mkdir(dirname(output), { recursive: true });
await writeFile(output, Buffer.from(screenshot.data, "base64"));
socket.close();

console.log(JSON.stringify({
  page: { title: page.title, url: page.url },
  renderer: {
    readyState: state.readyState,
    title: state.title,
    rootChildren: state.rootChildren,
    horizontalOverflow: state.horizontalOverflow,
  },
  proxy: state.proxy,
  manualProxyToggle,
  screenshot: output,
}, null, 2));
