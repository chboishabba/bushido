// Run: node --test scripts/linux/test-pip-policy.cjs
// Pure mocked DOM policy. This is not native video/GTK acceptance.
const test = require("node:test");
const assert = require("node:assert/strict");
const vm = require("node:vm");
const fs = require("node:fs");
const path = require("node:path");
const source = fs.readFileSync(path.join(__dirname, "../../src-tauri/src/linux_pip.js"), "utf8");
function fixture(opts = {}) {
  let host, button, calls = 0, exits = 0, reports = 0;
  const timers = [];
  const video = { isConnected: true, readyState: 2, videoWidth: 640,
    disablePictureInPicture: false, requestPictureInPicture() {
      calls++; return opts.deferred || (opts.reject ? Promise.reject(new Error("denied")) : Promise.resolve());
    }, ...opts.video };
  const document = {
    pictureInPictureEnabled: opts.enabled !== false,
    pictureInPictureElement: opts.active ? video : null,
    exitPictureInPicture() { exits++; return Promise.resolve(); },
    getElementById() { return host?.isConnected ? host : null; },
    querySelectorAll() { return [video]; },
    documentElement: { appendChild(node) { node.isConnected = true; } },
    createElement(tag) {
      const node = { style: {}, isConnected: false, disabled: false,
        remove() { this.isConnected = false; },
        attachShadow() { return {appendChild() {}}; },
        addEventListener(_, callback) { this.click = callback; }
      };
      if (tag === "div") host = node; else button = node;
      return node;
    }
  };
  const context = vm.createContext({document, window: {__bushidoReportVideo() { reports++; }},
    setTimeout(callback, delay) { timers.push({callback, delay}); }});
  return { video, document, timers, run: () => vm.runInContext(source, context),
    click: trusted => button.click({isTrusted: trusted, preventDefault() {}, stopPropagation() {}}),
    get calls() { return calls; }, get exits() { return exits; }, get reports() { return reports; },
    get host() { return host; }, get button() { return button; }};
}
const settle = () => new Promise(resolve => setImmediate(resolve));
test("unsupported engine never opens a prompt", () => {
  const f = fixture({enabled:false}); assert.equal(f.run(),"unsupported"); assert.equal(f.host,undefined);
});
test("disabled, not ready and audio-only video rejected", () => {
  for (const video of [{disablePictureInPicture:true},{readyState:0},{videoWidth:0},{isConnected:false}]) {
    const f=fixture({video}); assert.equal(f.run(),"no-eligible-video"); assert.equal(f.calls,0);
  }
});
test("trusted click requests exactly once, never synthetic", async () => {
  const f=fixture(); assert.equal(f.run(),"awaiting-gesture"); f.click(false); assert.equal(f.calls,0);
  f.click(true); f.click(true); await settle(); assert.equal(f.calls,1); assert.equal(f.host.isConnected,false);
});
test("eligibility rechecked at activation", async () => {
  const f=fixture(); f.run(); f.video.disablePictureInPicture=true; f.click(true); await settle();
  assert.equal(f.calls,0); assert.equal(f.button.textContent,"Picture-in-picture unavailable");
});
test("rejection does not claim active PiP", async () => {
  const f=fixture({reject:true}); f.run(); f.click(true); await settle();
  assert.equal(f.calls,1); assert.equal(f.button.textContent,"Picture-in-picture unavailable");
  assert.ok(f.reports>0); assert.equal(f.document.pictureInPictureElement,null);
});
test("duplicate toggle dismisses prompt and invalidates old click", () => {
  const f=fixture(); f.run(); assert.equal(f.run(),"dismissed"); f.click(true); assert.equal(f.calls,0);
});
test("active PiP can exit even if entry support disabled", async () => {
  const f=fixture({active:true,enabled:false}); assert.equal(f.run(),"exit-requested"); await settle();
  assert.equal(f.exits,1); assert.ok(f.reports>0); assert.equal(f.host,undefined);
});
test("abandoned prompt expires", () => {
  const f=fixture(); f.run(); f.timers.find(t=>t.delay===15000).callback(); f.click(true);
  assert.equal(f.host.isConnected,false); assert.equal(f.calls,0);
});
test("pending native request cannot be replaced by repeated toggle or timeout", async () => {
  let resolve;
  const deferred = new Promise(r => { resolve = r; });
  const f = fixture({deferred}); f.run(); f.click(true);
  assert.equal(f.run(),"request-pending");
  f.timers.find(t=>t.delay===15000).callback();
  assert.equal(f.run(),"request-pending"); f.click(true); assert.equal(f.calls,1);
  resolve(); await settle(); assert.equal(f.host.isConnected,false);
});
