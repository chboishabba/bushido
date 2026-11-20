(function() {
  if (window.__bushidoMedia) return;
  Object.defineProperty(window, '__bushidoMedia', { value: true, writable: false, configurable: false });
  var lastState = '';
  var lastTime = 0;
  // Query the actual document rather than infer PiP from a command attempt.
  function videoStatus() {
    var videos = document.querySelectorAll('video');
    var has = Array.prototype.some.call(videos, function(v) {
      return v.readyState >= 2 || !!v.currentSrc || !!v.src || !!v.srcObject || !!v.querySelector('source');
    });
    var supported = document.pictureInPictureEnabled === true &&
      Array.prototype.some.call(videos, function(v) {
        return typeof v.requestPictureInPicture === 'function' && !v.disablePictureInPicture;
      });
    return {__bushido: 'video', url: location.href, hasVideo: has,
      pipSupported: supported, pipActive: !!document.pictureInPictureElement};
  }
  function reportVideo() {
    var msg = videoStatus();
    try {
      if (window.webkit && window.webkit.messageHandlers &&
          window.webkit.messageHandlers.bushidoMedia) {
        window.webkit.messageHandlers.bushidoMedia.postMessage(JSON.stringify(msg));
      } else if (window.chrome && window.chrome.webview) {
        window.chrome.webview.postMessage(JSON.stringify(msg));
      }
    } catch (_) {}
  }
  window.__bushidoReportVideo = reportVideo;
  document.addEventListener('enterpictureinpicture', reportVideo, true);
  document.addEventListener('leavepictureinpicture', reportVideo, true);
  // Periodic snapshots recover from late registration and dropped events.
  setInterval(reportVideo, 1500);

  function getMetadata() {
    var meta = {};
    if (navigator.mediaSession && navigator.mediaSession.metadata) {
      var m = navigator.mediaSession.metadata;
      meta.artist = m.artist || '';
      meta.album = m.album || '';
      if (m.artwork && m.artwork.length > 0) meta.artwork = m.artwork[0].src;
      if (m.title) meta.metaTitle = m.title;
    }
    return meta;
  }

  function report(state, force) {
    var media = document.querySelector('video, audio');
    var msg = {
      __bushido: 'media',
      state: state,
      title: document.title,
      currentTime: media ? media.currentTime : 0,
      duration: media ? media.duration : 0,
      playbackRate: media ? media.playbackRate : 1
    };
    var metadata = getMetadata();
    if (metadata.artist) msg.artist = metadata.artist;
    if (metadata.album) msg.album = metadata.album;
    if (metadata.artwork) msg.artwork = metadata.artwork;
    if (metadata.metaTitle) msg.metaTitle = metadata.metaTitle;

    var stateKey = state + '|' + Math.floor(msg.currentTime);
    if (!force && stateKey === lastState) return;
    try {
      if (window.webkit && window.webkit.messageHandlers &&
          window.webkit.messageHandlers.bushidoMedia) {
        // Only forward fields consumed by the existing browser UI.
        delete msg.artwork;
        delete msg.album;
        msg.url = location.href;
        ['title', 'artist', 'metaTitle'].forEach(function(key) {
          if (typeof msg[key] === 'string') msg[key] = msg[key].slice(0, 256);
        });
        window.webkit.messageHandlers.bushidoMedia.postMessage(JSON.stringify(msg));
      } else if (window.chrome && window.chrome.webview) {
        window.chrome.webview.postMessage(JSON.stringify(msg));
      } else {
        return; // keep the state eligible for a later poll
      }
      lastState = stateKey;
    } catch (_) {}
  }

  // Event listeners on media elements
  function attachListeners(media) {
    if (media.__bushidoListeners) return;
    media.__bushidoListeners = true;

    media.addEventListener('play', function() { report('playing', true); });
    media.addEventListener('pause', function() { report('paused', true); });
    media.addEventListener('ended', function() { report('ended', true); });

    // throttled timeupdate (every 1s)
    media.addEventListener('timeupdate', function() {
      var now = Date.now();
      if (now - lastTime < 1000) return;
      lastTime = now;
      if (!media.paused && media.readyState > 0) {
        report('playing', false);
      }
    });
  }

  // Poll for media elements and attach listeners
  setInterval(function() {
    var media = document.querySelector('video, audio');
    if (!media) return;
    attachListeners(media);
    if (media.ended) {
      report('ended', false);
    } else if (!media.paused && media.readyState > 0) {
      report('playing', false);
    } else if (media.paused && media.readyState > 0 && media.currentTime > 0) {
      report('paused', false);
    }
  }, 1500);
})();
