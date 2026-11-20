// Installed in a named isolated world, on the current top-level document only.
function (channel, ticket, href) {
  "use strict";
  if (window !== window.top || !isSecureContext || location.protocol !== "https:" || location.href !== href) return false;
  if (globalThis.__bushidoSaveDispose) globalThis.__bushidoSaveDispose();
  const handler = window.webkit?.messageHandlers?.[channel];
  if (!handler) return false;
  let pending = null, timer = null, serial = 0, gesture = null;
  const typed = new WeakSet();
  const clear = () => { pending = null; if (timer !== null) clearTimeout(timer); timer = null; };
  const visible = el => el && el.isConnected && !el.disabled && !el.readOnly && el.getClientRects().length > 0 && getComputedStyle(el).visibility === "visible";
  const pair = form => {
    if (!form?.isConnected || form.method.toLowerCase() !== "post" || form.target) return null;
    let action;
    try { action = new URL(form.action, href); } catch (_) { return null; }
    if (action.origin !== location.origin || action.username || action.password) return null;
    const fields = Array.from(form.elements);
    if (fields.some(el => ["formaction", "formtarget", "formmethod"].some(a => el.hasAttribute(a)))) return null;
    const passwords = fields.filter(el => el instanceof HTMLInputElement && el.type === "password");
    const users = fields.filter(el => el instanceof HTMLInputElement && visible(el) && ["text", "email", "tel"].includes(el.type) && (el.type === "email" || ["username", "email"].includes(el.autocomplete) || /^(user(name)?|email|login)$/i.test(el.name)));
    if (passwords.length !== 1 || users.length !== 1 || !visible(passwords[0]) || passwords[0].autocomplete === "new-password") return null;
    return {user: users[0], pass: passwords[0], action: action.href};
  };
  const input = e => {
    if (pending) clear();
    if (e.isTrusted && e.target instanceof HTMLInputElement && e.target.type === "password") typed.add(e.target);
  };
  const click = e => {
    if (!e.isTrusted) return;
    const el = e.target?.closest?.("button,input");
    gesture = el?.form && ["submit", "image"].includes(el.type) ? {form: el.form, at: Date.now()} : null;
  };
  const key = e => {
    if (!e.isTrusted || e.key !== "Enter") return;
    gesture = e.target?.form ? {form: e.target.form, at: Date.now()} : null;
  };
  const submit = e => {
    const recent = gesture; gesture = null;
    if (!e.isTrusted || !recent || recent.form !== e.target || Date.now() - recent.at > 1500 || pending || location.href !== href) return;
    const fields = pair(e.target);
    if (!fields || !typed.has(fields.pass)) return;
    const username = fields.user.value, password = fields.pass.value;
    if (!username || username.length > 1024 || !password || password.length > 8192) return;
    pending = {id: ++serial, form: e.target, ...fields, username, password};
    timer = setTimeout(clear, 20000);
    try { handler.postMessage(JSON.stringify({ticket, href, id: serial, username})); } catch (_) { clear(); }
  };
  globalThis.__bushidoSaveTake = (secret, id) => {
    const saved = pending; clear();
    if (secret !== ticket || !saved || saved.id !== id || location.href !== href) return null;
    const fields = pair(saved.form);
    if (!fields || fields.user !== saved.user || fields.pass !== saved.pass || fields.action !== saved.action || fields.user.value !== saved.username || fields.pass.value !== saved.password) return null;
    return JSON.stringify({username: saved.username, password: saved.password});
  };
  const dispose = () => {
    clear(); gesture = null;
    for (const [type, fn] of events) document.removeEventListener(type, fn, true);
    window.removeEventListener("pagehide", dispose, true);
    delete globalThis.__bushidoSaveTake;
    delete globalThis.__bushidoSaveDispose;
  };
  const events = [["input", input], ["click", click], ["keydown", key], ["submit", submit]];
  for (const [type, fn] of events) document.addEventListener(type, fn, true);
  window.addEventListener("pagehide", dispose, true);
  globalThis.__bushidoSaveDispose = dispose;
  return true;
}
