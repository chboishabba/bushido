(function () {
  "use strict";
  // Only use the browser engine's actual presentation API. Never extract a
  // signed/media URL, replay the stream, or call an HTML overlay native PiP.
  const report = () => {
    try { if (typeof window.__bushidoReportVideo === "function") window.__bushidoReportVideo(); } catch (_) {}
  };
  const eligible = v => v && v.isConnected && v.readyState > 0 &&
    v.videoWidth > 0 && !v.disablePictureInPicture &&
    typeof v.requestPictureInPicture === "function";
  const existing = document.getElementById("__bushido_linux_pip_prompt");
  if (existing) {
    existing.remove();
    return "dismissed";
  }
  if (document.pictureInPictureElement) {
    if (typeof document.exitPictureInPicture !== "function") return "unsupported";
    try {
      Promise.resolve(document.exitPictureInPicture()).then(report, report);
      return "exit-requested";
    } catch (_) { return "failed"; }
  }
  if (document.pictureInPictureEnabled !== true) return "unsupported";
  const video = Array.from(document.querySelectorAll("video")).find(eligible);
  if (!video) return "no-eligible-video";

  const host = document.createElement("div");
  host.id = "__bushido_linux_pip_prompt";
  host.style.cssText = "position:fixed;inset:0;z-index:2147483647;pointer-events:none";
  const shadow = host.attachShadow({mode:"closed"});
  const button = document.createElement("button");
  button.type = "button";
  button.textContent = "Open picture-in-picture";
  button.style.cssText = "position:fixed;top:50%;left:50%;transform:translate(-50%,-50%);background:#191919;color:white;padding:14px 24px;border-radius:10px;border:1px solid #888;font:600 15px system-ui;cursor:pointer;pointer-events:auto";
  const dismiss = () => { host.remove(); report(); };
  const fail = () => {
    if (host.isConnected) {
      button.textContent = "Picture-in-picture unavailable";
      button.disabled = true;
      setTimeout(dismiss, 2000);
    }
    report();
  };
  button.addEventListener("click", event => {
    if (!event.isTrusted || !host.isConnected || button.disabled) return;
    event.preventDefault();
    event.stopPropagation();
    button.disabled = true;
    if (document.pictureInPictureEnabled !== true || !eligible(video) ||
        document.pictureInPictureElement) { fail(); return; }
    try {
      Promise.resolve(video.requestPictureInPicture()).then(dismiss, fail);
    } catch (_) { fail(); }
  });
  shadow.appendChild(button);
  document.documentElement.appendChild(host);
  setTimeout(dismiss, 15000);
  return "awaiting-gesture";
})()
