// Ratspeak-ru: text size presets below 100% (50 / 70 / 90).
// Upstream text_scale.js and set_text_scale clamp to 100..140, so the fork keeps the
// sub-100 choice in its own localStorage key and wraps RS.textScale at load time.
// No upstream file is edited; upstream still stores 100 in its own settings.
(function() {
    'use strict';

    var RS = window.RS;
    var base = RS && RS.textScale;
    if (!base || base.__ruWrapped) return;

    var KEY = 'ratspeak-ru-text-scale';
    var SMALL = [50, 70, 90];

    function readSmall() {
        try {
            var v = Number(localStorage.getItem(KEY));
            return SMALL.indexOf(v) !== -1 ? v : null;
        } catch (_) { return null; }
    }
    function writeSmall(v) {
        try {
            if (v === null) localStorage.removeItem(KEY);
            else localStorage.setItem(KEY, String(v));
        } catch (_) {}
    }
    function nearestSmall(value) {
        var n = Number(value);
        if (!Number.isFinite(n) || n >= 100) return null;
        var best = SMALL[0];
        SMALL.forEach(function(c) { if (Math.abs(c - n) < Math.abs(best - n)) best = c; });
        return best;
    }
    function applySmall(percent, announce) {
        var root = document.documentElement;
        root.style.fontSize = percent + '%';
        root.dataset.textScale = String(percent);
        root.dataset.textScaleTier = percent <= 70 ? 'small' : 'compact';
        if (announce && typeof window.CustomEvent === 'function') {
            window.dispatchEvent(new CustomEvent('ratspeak-text-scale-changed', {
                detail: { percent: percent, committed: true }
            }));
        }
        return percent;
    }

    var wrapped = Object.assign({}, base, {
        __ruWrapped: true,
        MIN: 50,
        normalize: function(value) {
            var small = nearestSmall(value);
            return small !== null ? small : base.normalize(value);
        },
        get: function() {
            var small = readSmall();
            return small !== null ? small : base.get();
        },
        preview: function(value) {
            var small = nearestSmall(value);
            return small !== null ? applySmall(small, false) : base.preview(value);
        },
        commit: function(value) {
            var small = nearestSmall(value);
            if (small !== null) { writeSmall(small); return applySmall(small, true); }
            // Backend echoes 100 after a small choice; keep the fork value then.
            var kept = readSmall();
            if (kept !== null && Number(value) === 100) return applySmall(kept, true);
            writeSmall(null);
            return base.commit(value);
        },
        reset: function() { writeSmall(null); return base.reset(); }
    });
    RS.textScale = wrapped;

    // A tap on a real >=100 preset must clear the fork value before upstream's own
    // handler runs (capture phase), otherwise the echoed 100 would be ignored.
    document.addEventListener('change', function(event) {
        var t = event.target;
        if (!t || t.name !== 'settings-text-scale' || !t.checked) return;
        if (Number(t.value) >= 100) writeSmall(null);
    }, true);

    var initial = readSmall();
    if (initial !== null) applySmall(initial, true);
})();
