(function() {
    'use strict';

    var STORAGE_KEY = 'rs-text-scale-percent';
    var MIN = 50;
    var DEFAULT = 100;
    var MAX = 140;
    var STEP = 10;
    // Допустимые значения: 50/70/90 (уменьшение) и 100..140 с шагом 10.
    var ALLOWED = [50, 70, 90, 100, 110, 120, 130, 140];

    function normalize(value) {
        var numeric = Number(value);
        if (!Number.isFinite(numeric)) numeric = DEFAULT;
        numeric = Math.max(MIN, Math.min(MAX, numeric));
        var best = ALLOWED[0];
        for (var i = 1; i < ALLOWED.length; i++) {
            if (Math.abs(ALLOWED[i] - numeric) < Math.abs(best - numeric)) best = ALLOWED[i];
        }
        return best;
    }

    function storedValue() {
        try {
            var raw = localStorage.getItem(STORAGE_KEY);
            if (raw !== null) return normalize(raw);
        } catch (_) {}
        return DEFAULT;
    }

    function tier(percent) {
        if (percent <= 70) return 'small';
        if (percent < 100) return 'compact';
        if (percent >= 140) return 'xlarge';
        if (percent >= 130) return 'large';
        if (percent >= 120) return 'medium';
        return 'normal';
    }

    function apply(value, committed, announce) {
        var percent = normalize(value);
        var root = document.documentElement;
        root.style.fontSize = percent + '%';
        root.dataset.textScale = String(percent);
        root.dataset.textScaleTier = tier(percent);

        if (committed) {
            try {
                if (percent === DEFAULT) localStorage.removeItem(STORAGE_KEY);
                else localStorage.setItem(STORAGE_KEY, String(percent));
            } catch (_) {}
        }

        if (announce !== false && typeof window.CustomEvent === 'function') {
            window.dispatchEvent(new CustomEvent('ratspeak-text-scale-changed', {
                detail: { percent: percent, committed: !!committed }
            }));
        }
        return percent;
    }

    window.RS = window.RS || {};
    window.RS.textScale = {
        MIN: MIN,
        DEFAULT: DEFAULT,
        ALLOWED: ALLOWED,
        MAX: MAX,
        STEP: STEP,
        get: function() {
            return normalize(document.documentElement.dataset.textScale || storedValue());
        },
        preview: function(value) { return apply(value, false, true); },
        commit: function(value) { return apply(value, true, true); },
        reset: function() { return apply(DEFAULT, true, true); },
        normalize: normalize
    };

    // Apply before CSS and fonts load so the first rendered frame is already
    // at the user's chosen size.
    apply(storedValue(), false, false);
})();
