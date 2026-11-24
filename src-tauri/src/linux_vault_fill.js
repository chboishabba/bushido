// Function evaluated only in the named isolated WebKit script world.
// Never accept a page message or export credentials on window.
function(mode, ticket, origin, href, username, password) {
  "use strict";
  if (location.origin !== origin || location.href !== href || window !== window.top ||
      location.protocol !== "https:" || !isSecureContext) return false;
  const visible = el => el && el.isConnected && !el.disabled && !el.readOnly &&
    el.getClientRects().length > 0 && getComputedStyle(el).visibility === "visible";
  const safeForm = form => {
    if (!form || !form.isConnected || form.method.toLowerCase() !== "post") return false;
    try {
      const action = new URL(form.action, location.href);
      return action.origin === origin && action.protocol === "https:" &&
        !action.username && !action.password && !form.target &&
        !form.querySelector("[formaction], [formtarget], [formmethod]");
    } catch (_) { return false; }
  };
  const eligible = () => {
    const pairs = [];
    for (const form of document.forms) {
      if (!safeForm(form)) continue;
      const passwords = Array.from(form.elements).filter(el => el instanceof HTMLInputElement &&
        el.type === "password" && visible(el));
      if (passwords.length !== 1 || passwords[0].autocomplete === "new-password") continue;
      const users = Array.from(form.elements).filter(el => el instanceof HTMLInputElement &&
        ["text", "email", "tel"].includes(el.type) && visible(el) &&
        (el.autocomplete === "username" || el.autocomplete === "email" ||
          el.type === "email" || /^(user(name)?|email|login)$/i.test(el.name)));
      if (users.length === 1 && !passwords[0].value && !users[0].value)
        pairs.push({form, user: users[0], pass: passwords[0]});
    }
    return pairs.length === 1 ? pairs[0] : null;
  };
  if (mode === "probe") {
    globalThis.__bushidoNativeVaultTicket = null;
    const pair = eligible();
    if (!pair) return false;
    globalThis.__bushidoNativeVaultTicket = {ticket, document, ...pair};
    return true;
  }
  const saved = globalThis.__bushidoNativeVaultTicket;
  globalThis.__bushidoNativeVaultTicket = null;
  if (mode !== "fill" || !saved || saved.ticket !== ticket || saved.document !== document)
    return false;
  const pair = eligible();
  if (!pair || pair.form !== saved.form || pair.user !== saved.user || pair.pass !== saved.pass)
    return false;
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set;
  setter.call(pair.user, username);
  setter.call(pair.pass, password);
  // Both values intentionally become readable to this explicitly authorized origin.
  for (const field of [pair.user, pair.pass]) {
    field.dispatchEvent(new Event("input", {bubbles:true}));
    field.dispatchEvent(new Event("change", {bubbles:true}));
  }
  return true; // never submit the form
}
