(function() {
    'use strict';

    // Percent of the standard type scale; 100 is the platform default size.
    var STORAGE_KEY = 'rs-text-scale';
    var LEGACY_STORAGE_KEY = 'rs-text-scale-percent';
    var MIN = 90;
    var DEFAULT = 100;
    var MAX = 150;
    var STEP = 10;

    function normalize(value) {
        var numeric = value === null || value === undefined || value === '' ? NaN : Number(value);
        if (!Number.isFinite(numeric)) numeric = DEFAULT;
        numeric = Math.max(MIN, Math.min(MAX, numeric));
        return Math.round((numeric - MIN) / STEP) * STEP + MIN;
    }

    // Presets before the standard scale were one step larger for the same
    // size; move them down one step so nobody ends up smaller than chosen.
    function migrateLegacy() {
        var legacy = localStorage.getItem(LEGACY_STORAGE_KEY);
        if (legacy === null) return;
        localStorage.removeItem(LEGACY_STORAGE_KEY);
        var migrated = Math.max(DEFAULT, normalize(Number(legacy) - STEP));
        if (migrated !== DEFAULT) localStorage.setItem(STORAGE_KEY, String(migrated));
    }

    function storedValue() {
        try {
            migrateLegacy();
            return normalize(localStorage.getItem(STORAGE_KEY));
        } catch (_) {}
        return DEFAULT;
    }

    function tier(percent) {
        if (percent >= 130) return 'xlarge';
        if (percent >= 120) return 'large';
        if (percent >= 110) return 'medium';
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
