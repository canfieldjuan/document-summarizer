// Execute the shipped frontend, not copies of its handlers. The fixture supplies
// the DOM/IPC boundary only; native ownership/projection is covered in Rust.
import { readFileSync } from "node:fs";
import { setImmediate } from "node:timers/promises";
import vm from "node:vm";
import assert from "node:assert/strict";
import test from "node:test";
import ts from "typescript";

const source = readFileSync(new URL("./main.ts", import.meta.url), "utf8");
const script = new vm.Script(ts.transpileModule(source, {
  compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
}).outputText, { filename: "actual-main.ts" });
const html = readFileSync(new URL("../index.html", import.meta.url), "utf8");

class Element {
  hidden = false; disabled = false; textContent = ""; value = "";
  children = []; dataset = {}; listeners = new Map();
  classList = { toggle() {} };
  addEventListener(name, fn) { this.listeners.set(name, fn); }
  append(...children) { this.children.push(...children); }
  prepend(...children) { this.children.unshift(...children); }
  replaceChildren(...children) { this.children = children; this.textContent = ""; }
  querySelector() { return new Element(); }
  querySelectorAll() { return []; }
  click() { assert.equal(this.disabled, false); this.listeners.get("click")?.(); }
}

async function eventually(predicate, message) {
  for (let i = 0; i < 200; i++) {
    if (predicate()) return;
    await setImmediate();
  }
  assert.fail(message);
}

async function app() {
  const nodes = new Map([...html.matchAll(/id="([^"]+)"/g)].map((m) => [m[1], new Element()]));
  const node = (id) => { assert.ok(nodes.has(id), `real DOM id ${id} exists`); return nodes.get(id); };
  node("summary-profile").value = "general";
  const state = { ownerAlive: true, code: "DOCUMENT_STORE_UNAVAILABLE", calls: [], statuses: [] };
  const status = () => ({ providerName: "Public fixture", ready: state.ownerAlive,
    code: state.ownerAlive ? null : state.code, message: state.ownerAlive ? "Ready" : "Public store unavailable", recoverable: !state.ownerAlive });
  const invoke = async (command) => {
    state.calls.push(command);
    if (command === "get_model_catalog") return { selectedSource: "gateway", selectedPresetId: "", presets: [], installedModels: [], gateway: { configured: true, platformSupported: true, baseUrl: "https://public-fixture.invalid" } };
    if (command === "get_runtime_status") return state.statuses.length ? state.statuses.shift() : status();
    if (command === "get_connect_entitlement_status") throw { code: "PUBLIC_PROBE_UNUSED", message: "Connect not used" };
    if (command === "retry_document_store") { state.ownerAlive = true; return; }
    if (command === "list_recent_runs" && state.ownerAlive) return [];
    if (command === "list_recent_runs" || command === "summarize_document") throw { code: state.code, message: "Public store unavailable" };
    assert.fail(`unexpected command ${command}`);
  };
  const context = vm.createContext({ exports: {}, console, setTimeout,
    require: (name) => name === "@tauri-apps/api/core" ? { invoke } : name === "@tauri-apps/plugin-dialog" ? { open: async () => "/public-fixture.pdf" } : assert.fail(name),
    document: { querySelector: (selector) => nodes.get(selector.slice(1)), createElement: () => new Element(), addEventListener() {}, visibilityState: "visible" },
    window: { addEventListener() {}, setInterval() {}, setTimeout },
  });
  script.runInContext(context);
  await eventually(() => node("runtime-title").textContent.endsWith("ready") && state.calls.includes("list_recent_runs") && !node("history-refresh").disabled, "initial frontend ready");
  assert.equal(node("runtime-retry").hidden, true);
  return { state, node, context, statusCount: () => state.calls.filter((x) => x === "get_runtime_status").length };
}

for (const action of ["history-refresh", "select-btn"]) {
  test(`departed owner: ${action} exposes existing Retry and explicit Retry reconnects`, async () => {
    const a = await app();
    a.state.ownerAlive = false;
    a.node(action).click();
    await eventually(() => !a.node("runtime-retry").hidden, "owner loss must reveal Retry");
    assert.equal(a.node("select-btn").disabled, true, "unavailable storage disables new work");
    assert.ok(a.statusCount() > 1, "command rejection must refresh runtime status");
    assert.equal(a.state.calls.includes("retry_document_store"), false, "ordinary failures never bootstrap");
    await eventually(() => a.node("history-status").textContent === "Public store unavailable" && !a.node("history-refresh").disabled, "original rejection reaches local presentation");
    a.node("runtime-retry").click();
    await eventually(() => a.state.ownerAlive && a.node("runtime-retry").hidden && !a.node("runtime-retry").disabled, "explicit Retry reconnects");
  });
}

for (const code of ["PROVIDER_STORE_BUSY", "PIPELINE_STORE_ERROR", "BACKGROUND_STALE_STATE"]) {
  test(`${code} retains the local error without declaring storage unavailable`, async () => {
    const a = await app(); a.state.ownerAlive = false; a.state.code = code;
    a.node("history-refresh").click();
    await eventually(() => !a.node("history-refresh").disabled, "failed history action settles");
    assert.equal(a.node("history-status").textContent, "Public store unavailable");
    assert.equal(a.statusCount(), 1);
    assert.equal(a.node("runtime-retry").hidden, true);
  });
}

test("a late successful status cannot hide the newer owner-loss Retry", async () => {
  const a = await app(); let finishOldStatus;
  a.state.statuses.push(new Promise((resolve) => { finishOldStatus = resolve; }));
  const older = vm.runInContext("refreshRuntimeStatus()", a.context);
  a.state.ownerAlive = false; a.node("history-refresh").click();
  await eventually(() => a.statusCount() === 3 && !a.node("runtime-retry").hidden, "new unavailable status wins");
  finishOldStatus({ providerName: "Stale", ready: true, message: "Ready" });
  await older;
  assert.equal(a.node("runtime-retry").hidden, false);
  assert.equal(a.node("select-btn").disabled, true);
  assert.notEqual(a.node("runtime-title").textContent, "Stale ready");
});

test("a failed runtime-status probe cannot recurse through the command-error owner", async () => {
  const a = await app();
  // Reject when consumed, avoiding an unhandled fixture promise before the call.
  a.state.statuses.push({ then(_resolve, reject) { reject({ code: "DOCUMENT_STORE_UNAVAILABLE", message: "Probe unavailable" }); } });
  a.state.ownerAlive = false; a.node("history-refresh").click();
  await eventually(() => !a.node("runtime-retry").hidden && !a.node("history-refresh").disabled, "failed status still exposes Retry");
  assert.equal(a.statusCount(), 2, "one refresh, no recursive command retry");
});
