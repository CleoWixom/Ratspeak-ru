// Ratspeak-ru: GNSS position source (setting "Location (GNSS)").
// Phone fixes come from navigator.geolocation (the Android WebView already has geolocation
// enabled and ACCESS_FINE_LOCATION declared). A radio device can feed fixes through
// RuGeo.pushRadioFix() once its firmware reports them. The last fix stays in memory only;
// only the on/off switch and the chosen source are stored. Sharing and the map build on this.
(function() {
    'use strict';

    var KEY = 'ratspeak-ru-geo';
    var MIN_INTERVAL_MS = 5000;
    var MIN_MOVE_M = 10;
    var SOURCES = ['phone', 'radio'];

    var TEXT = {
        en: { label: 'Location (GNSS)', desc: 'Let Ratspeak read your position. Nothing is sent until you share it.',
              phone: 'This phone', radio: 'Radio device', source: 'Position source',
              off: 'Off', waiting: 'Waiting for a fix…', radioWaiting: 'Waiting for the radio device to report a position',
              denied: 'Location permission was denied', unavailable: 'Location is not available on this device',
              fix: 'Accuracy ±{m} m · {s} s ago', fixNoAcc: 'Position known · {s} s ago', error: 'Could not get a position' },
        ru: { label: 'Местоположение (GNSS)', desc: 'Разрешить Ratspeak читать вашу позицию. Ничего не отправляется, пока вы не поделитесь ею.',
              phone: 'Этот телефон', radio: 'Радиоустройство', source: 'Источник позиции',
              off: 'Выключено', waiting: 'Ожидание координат…', radioWaiting: 'Ожидание позиции от радиоустройства',
              denied: 'Доступ к местоположению запрещён', unavailable: 'Местоположение недоступно на этом устройстве',
              fix: 'Точность ±{m} м · {s} с назад', fixNoAcc: 'Позиция известна · {s} с назад', error: 'Не удалось получить позицию' }
    };

    function lang() {
        var l = 'en';
        try { l = localStorage.getItem('ratspeak_lang') || (navigator.language || 'en').slice(0, 2); } catch (_) {}
        return l === 'ru' ? 'ru' : 'en';
    }
    function t(key, vars) {
        var s = (TEXT[lang()] || TEXT.en)[key] || TEXT.en[key] || key;
        Object.keys(vars || {}).forEach(function(k) { s = s.replace('{' + k + '}', vars[k]); });
        return s;
    }

    function readConfig() {
        var cfg = { enabled: false, source: 'phone' };
        try {
            var parsed = JSON.parse(localStorage.getItem(KEY) || '{}');
            if (parsed && parsed.enabled === true) cfg.enabled = true;
            if (parsed && SOURCES.indexOf(parsed.source) !== -1) cfg.source = parsed.source;
        } catch (_) {}
        return cfg;
    }
    function writeConfig(cfg) {
        try { localStorage.setItem(KEY, JSON.stringify(cfg)); } catch (_) {}
    }

    function num(v) { return typeof v === 'number' && Number.isFinite(v) ? v : null; }

    // Pure: validate and normalize a fix; returns null for anything out of range.
    function normalizeFix(raw, source, now) {
        if (!raw) return null;
        var lat = num(raw.latitude !== undefined ? raw.latitude : raw.lat);
        var lon = num(raw.longitude !== undefined ? raw.longitude : raw.lon);
        if (lat === null || lon === null || lat < -90 || lat > 90 || lon < -180 || lon > 180) return null;
        return {
            lat: lat, lon: lon,
            alt: num(raw.altitude !== undefined ? raw.altitude : raw.alt),
            acc: num(raw.accuracy !== undefined ? raw.accuracy : raw.acc),
            speed: num(raw.speed),
            heading: num(raw.heading),
            source: source,
            ts: now
        };
    }

    // Pure: great-circle distance in meters.
    function distanceM(a, b) {
        var R = 6371000, rad = Math.PI / 180;
        var dLat = (b.lat - a.lat) * rad, dLon = (b.lon - a.lon) * rad;
        var h = Math.sin(dLat / 2) * Math.sin(dLat / 2) +
            Math.cos(a.lat * rad) * Math.cos(b.lat * rad) * Math.sin(dLon / 2) * Math.sin(dLon / 2);
        return 2 * R * Math.asin(Math.min(1, Math.sqrt(h)));
    }

    // Pure: keep updates at least 5 s apart unless the position moved 10 m or more.
    function shouldAccept(prev, next) {
        if (!prev) return true;
        if (next.ts - prev.ts >= MIN_INTERVAL_MS) return true;
        return distanceM(prev, next) >= MIN_MOVE_M;
    }

    var config = readConfig();
    var lastFix = null;
    var watchId = null;
    var listeners = [];
    var statusText = '';

    function emit() {
        listeners.slice().forEach(function(cb) { try { cb(lastFix, config); } catch (_) {} });
        if (typeof window.CustomEvent === 'function') {
            window.dispatchEvent(new CustomEvent('ratspeak-ru-geo', { detail: { fix: lastFix, config: config } }));
        }
        renderStatus();
    }

    function accept(raw, source) {
        var fix = normalizeFix(raw, source, Date.now());
        if (!fix || !shouldAccept(lastFix, fix)) return false;
        lastFix = fix;
        emit();
        return true;
    }

    function stopWatch() {
        if (watchId !== null && navigator.geolocation) navigator.geolocation.clearWatch(watchId);
        watchId = null;
    }

    function toast(message) {
        if (typeof window.showToast === 'function') window.showToast(message, 'toast-error', 4000);
    }

    function startWatch() {
        stopWatch();
        if (!config.enabled || config.source !== 'phone') return;
        if (!navigator.geolocation) {
            toast(t('unavailable'));
            setEnabled(false);
            return;
        }
        statusText = t('waiting');
        watchId = navigator.geolocation.watchPosition(function(position) {
            var c = position && position.coords;
            accept(c ? { latitude: c.latitude, longitude: c.longitude, altitude: c.altitude,
                         accuracy: c.accuracy, speed: c.speed, heading: c.heading } : null, 'phone');
        }, function(error) {
            if (error && error.code === 1) { // PERMISSION_DENIED
                toast(t('denied'));
                setEnabled(false);
            } else {
                statusText = t('error');
                renderStatus();
            }
        }, { enableHighAccuracy: true, maximumAge: 10000, timeout: 30000 });
    }

    function setEnabled(on) {
        config.enabled = !!on;
        writeConfig(config);
        if (!config.enabled) { stopWatch(); lastFix = null; }
        else startWatch();
        syncControls();
        emit();
    }

    function setSource(source) {
        if (SOURCES.indexOf(source) === -1) return;
        config.source = source;
        writeConfig(config);
        stopWatch();
        lastFix = null;
        statusText = config.enabled ? (source === 'radio' ? t('radioWaiting') : t('waiting')) : '';
        if (config.enabled) startWatch();
        syncControls();
        emit();
    }

    function renderStatus() {
        var el = document.getElementById('ru-geo-status');
        if (!el) return;
        if (!config.enabled) { el.textContent = t('off'); return; }
        if (lastFix) {
            var s = Math.max(0, Math.round((Date.now() - lastFix.ts) / 1000));
            el.textContent = lastFix.acc !== null ? t('fix', { m: Math.round(lastFix.acc), s: s })
                                                  : t('fixNoAcc', { s: s });
        } else {
            el.textContent = statusText || (config.source === 'radio' ? t('radioWaiting') : t('waiting'));
        }
    }

    function syncControls() {
        var toggle = document.getElementById('ru-geo-toggle');
        var select = document.getElementById('ru-geo-source');
        if (toggle) toggle.checked = config.enabled;
        if (select) { select.value = config.source; select.disabled = !config.enabled; }
        renderStatus();
    }

    function rowHtml() {
        return '<div class="settings-row" id="ru-geo-row">' +
            '<div class="settings-row-info">' +
                '<span class="settings-row-label">' + t('label') + '</span>' +
                '<span class="settings-row-desc">' + t('desc') + '</span>' +
                '<span class="settings-row-desc" id="ru-geo-status"></span>' +
            '</div>' +
            '<select id="ru-geo-source" class="nr-input-sm" aria-label="' + t('source') + '">' +
                '<option value="phone">' + t('phone') + '</option>' +
                '<option value="radio">' + t('radio') + '</option>' +
            '</select>' +
            '<label class="prop-toggle" aria-label="' + t('label') + '">' +
                '<input type="checkbox" id="ru-geo-toggle"><span class="prop-slider"></span>' +
            '</label>' +
        '</div>';
    }

    function buildSettings() {
        if (document.getElementById('ru-geo-row')) return;
        var anchor = document.getElementById('haptics-enabled-toggle');
        var row = anchor && anchor.closest('.settings-row');
        if (!row) return;
        row.insertAdjacentHTML('afterend', rowHtml());
        document.getElementById('ru-geo-toggle').addEventListener('change', function(e) { setEnabled(e.target.checked); });
        document.getElementById('ru-geo-source').addEventListener('change', function(e) { setSource(e.target.value); });
        syncControls();
    }

    window.RuGeo = {
        get: function() { return lastFix; },
        config: function() { return { enabled: config.enabled, source: config.source }; },
        setEnabled: setEnabled,
        setSource: setSource,
        onChange: function(cb) { if (typeof cb === 'function') listeners.push(cb); },
        // A radio device that reports GNSS calls this ({lat, lon, alt?, acc?, speed?, heading?}).
        pushRadioFix: function(raw) {
            if (!config.enabled || config.source !== 'radio') return false;
            return accept(raw, 'radio');
        },
        _pure: { normalizeFix: normalizeFix, distanceM: distanceM, shouldAccept: shouldAccept }
    };

    function init() {
        buildSettings();
        if (config.enabled) startWatch();
        setInterval(renderStatus, 5000);
    }
    if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', init);
    else init();
})();
