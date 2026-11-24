(function() {
    'use strict';
    if (window.__bushidoCosmeticObs) return;
    Object.defineProperty(window, '__bushidoCosmeticObs', { value: true, writable: false, configurable: false });

    var sentClasses = new Set();
    var sentIds = new Set();
    var pendingClasses = [];
    var pendingIds = [];
    var styleEl = null;
    var appliedSelectors = new Set();
    var flushTimer = null;
    var MAX_BATCH = 64;
    var MAX_TRACKED = 16384;

    function collectFromElement(el) {
        if (!el || el.nodeType !== 1) return;
        if (el.classList) {
            for (var i = 0; i < el.classList.length; i++) {
                var c = el.classList[i];
                if (c && c.length <= 128 && sentClasses.size < MAX_TRACKED && !sentClasses.has(c)) {
                    sentClasses.add(c);
                    pendingClasses.push(c);
                }
            }
        }
        if (el.id && el.id.length <= 128 && sentIds.size < MAX_TRACKED && !sentIds.has(el.id)) {
            sentIds.add(el.id);
            pendingIds.push(el.id);
        }
    }

    function collectAll(root) {
        try {
            var all = root.querySelectorAll('[class],[id]');
            for (var i = 0; i < all.length; i++) {
                collectFromElement(all[i]);
            }
        } catch(e) {}
    }

    function flush() {
        if (!pendingClasses.length && !pendingIds.length) return;
        var msg = {
            __bushido: 'cosmetic-probe',
            url: location.href,
            classes: pendingClasses.slice(0, MAX_BATCH),
            ids: pendingIds.slice(0, MAX_BATCH)
        };
        // JSON escaping can expand a token: bound actual UTF-8 bytes too.
        var raw = JSON.stringify(msg);
        while (new TextEncoder().encode(raw).length > 32768) {
            if (msg.classes.length + msg.ids.length <= 1) {
                pendingClasses.splice(0, msg.classes.length);
                pendingIds.splice(0, msg.ids.length);
                scheduleFlush();
                return;
            }
            if (msg.classes.length >= msg.ids.length) msg.classes.pop();
            else msg.ids.pop();
            raw = JSON.stringify(msg);
        }
        try {
            if (window.webkit && window.webkit.messageHandlers &&
                window.webkit.messageHandlers.bushidoCosmetic) {
                window.webkit.messageHandlers.bushidoCosmetic.postMessage(raw);
            } else if (window.chrome && window.chrome.webview) {
                window.chrome.webview.postMessage(raw);
            } else {
                scheduleFlush();
                return;
            }
            pendingClasses.splice(0, msg.classes.length);
            pendingIds.splice(0, msg.ids.length);
        } catch(e) {}
        if (pendingClasses.length || pendingIds.length) scheduleFlush();
    }

    function scheduleFlush() {
        if (flushTimer) return;
        flushTimer = setTimeout(function() {
            flushTimer = null;
            flush();
        }, 500);
    }

    // Called by Rust via wv.eval() / ExecuteScript when selectors arrive
    window.__bushidoApplyCosmetic = function(selectors) {
        if (!selectors || selectors.length === 0) return;
        var newSels = [];
        for (var i = 0; i < selectors.length; i++) {
            if (!appliedSelectors.has(selectors[i])) {
                appliedSelectors.add(selectors[i]);
                newSels.push(selectors[i]);
            }
        }
        if (newSels.length === 0) return;
        if (!styleEl) {
            styleEl = document.createElement('style');
            styleEl.id = 'bushido-generic-cosmetic';
            (document.head || document.documentElement).appendChild(styleEl);
        }
        styleEl.textContent += newSels.join(',') + '{display:none!important}\n';
    };

    // Initial collection from existing DOM
    if (document.documentElement) {
        collectAll(document.documentElement);
        scheduleFlush();
    }

    // Observe mutations for new elements
    var observer = new MutationObserver(function(mutations) {
        var dirty = false;
        for (var i = 0; i < mutations.length; i++) {
            var m = mutations[i];
            for (var j = 0; j < m.addedNodes.length; j++) {
                var node = m.addedNodes[j];
                if (node.nodeType === 1) {
                    collectFromElement(node);
                    if (node.querySelectorAll) {
                        collectAll(node);
                    }
                    dirty = true;
                }
            }
            if (m.type === 'attributes' && m.target && m.target.nodeType === 1) {
                collectFromElement(m.target);
                dirty = true;
            }
        }
        if (dirty && (pendingClasses.length > 0 || pendingIds.length > 0)) {
            scheduleFlush();
        }
    });

    observer.observe(document.documentElement || document, {
        childList: true,
        subtree: true,
        attributes: true,
        attributeFilter: ['class', 'id']
    });
})();
