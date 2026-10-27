// Announce propagation radar (Network → Activity).
//
// This node sits in the centre. Each ring is one hop, each sector is one
// interface the path table says a destination was heard through, and each dot
// is a known destination. When a new announce arrives it radiates a wave from
// its dot and a pulse travels inward across the hop rings to this node.
// Data comes only from what the app already has: the path table
// (hash, hops, interface, timestamp) and the `announce_received` event.
(function() {
    'use strict';

    var MAX_POINTS = 400;          // newest destinations drawn, to keep a busy hub smooth
    var MAX_PULSES = 24;
    var WAVE_MS = 1800;
    var TRAVEL_MS = 1300;
    var FRESH_MS = 5 * 60 * 1000;  // announce-only dots stay until the path table lists them
    var UNKNOWN = 'Unknown';
    var COLORS = { lora: '#f0883e', tcp: '#58a6ff', local: '#3fb950', ble: '#a371f7', other: '#8b949e' };

    // ---- Pure geometry (unit-tested) ---------------------------------------

    function classify(name, record) {
        var kind = (record && (record.type || record.iface_type || record.ifaceType)) || '';
        var text = (String(kind) + ' ' + String(name || '')).toLowerCase();
        if (/rnode|lora|radio/.test(text)) return 'lora';
        if (/tcp|backbone|internet/.test(text)) return 'tcp';
        if (/ble|bluetooth/.test(text)) return 'ble';
        if (/auto|local|udp/.test(text)) return 'local';
        return 'other';
    }

    // Two stable fractions in [0, 1) from a destination hash.
    function fractions(hash) {
        var hex = String(hash || '').replace(/[^0-9a-fA-F]/g, '');
        var a = parseInt(hex.slice(0, 8) || '0', 16) / 0x100000000;
        var b = parseInt(hex.slice(8, 16) || hex.slice(0, 8) || '0', 16) / 0x100000000;
        return [a || 0, b || 0];
    }

    function clamp(value, lo, hi) { return Math.max(lo, Math.min(hi, value)); }

    // entries: [{ hash, hops, iface, ts }]; records: name → interface record.
    function layout(entries, width, height, records) {
        var list = (entries || []).filter(function(e) { return e && e.hash; })
            .sort(function(a, b) { return (b.ts || 0) - (a.ts || 0); })
            .slice(0, MAX_POINTS);
        var maxHops = 3;
        list.forEach(function(e) {
            var hops = parseInt(e.hops, 10);
            if (hops > maxHops) maxHops = hops;
        });
        maxHops = Math.min(maxHops, 10);

        var counts = Object.create(null);
        list.forEach(function(e) {
            var name = e.iface || UNKNOWN;
            counts[name] = (counts[name] || 0) + 1;
        });
        var names = Object.keys(counts).sort(function(a, b) { return counts[b] - counts[a] || (a < b ? -1 : 1); });

        var cx = width / 2, cy = height / 2;
        var radius = Math.max(20, Math.min(width, height) / 2 - 18);
        var sectorSize = names.length ? (2 * Math.PI) / names.length : 2 * Math.PI;
        var sectors = names.map(function(name, i) {
            return { name: name, start: -Math.PI / 2 + i * sectorSize, size: sectorSize, count: counts[name],
                color: COLORS[classify(name, records && records[name])] };
        });
        var bySector = Object.create(null);
        sectors.forEach(function(s) { bySector[s.name] = s; });

        var gap = radius / maxHops;
        var points = list.map(function(e) {
            var sector = bySector[e.iface || UNKNOWN];
            var f = fractions(e.hash);
            var hops = clamp(parseInt(e.hops, 10) || 1, 1, maxHops);
            var margin = sector.size * 0.12;
            var angle = sector.start + margin + f[0] * (sector.size - 2 * margin);
            var r = clamp(hops * gap + (f[1] - 0.5) * gap * 0.5, gap * 0.45, radius);
            return { hash: e.hash, hops: hops, iface: sector.name, color: sector.color, fresh: !!e.fresh,
                angle: angle, r: r, x: cx + Math.cos(angle) * r, y: cy + Math.sin(angle) * r };
        });
        return { cx: cx, cy: cy, radius: radius, maxHops: maxHops, gap: gap, sectors: sectors, points: points };
    }

    function nearestPoint(model, x, y, within) {
        var best = null, bestDistance = within * within;
        (model && model.points || []).forEach(function(p) {
            var d = (p.x - x) * (p.x - x) + (p.y - y) * (p.y - y);
            if (d <= bestDistance) { best = p; bestDistance = d; }
        });
        return best;
    }

    // Wave and travelling pulse progress for one announce, both 0..1 (or null when over).
    function pulseState(ageMs) {
        if (ageMs < 0 || ageMs > Math.max(WAVE_MS, TRAVEL_MS)) return null;
        return { wave: ageMs <= WAVE_MS ? ageMs / WAVE_MS : null,
            travel: ageMs <= TRAVEL_MS ? ageMs / TRAVEL_MS : null };
    }

    // ---- State + DOM -------------------------------------------------------

    var paths = [];          // from the path table
    var recent = Object.create(null);   // hash → { hops, iface, ts, name }, from announce events
    var announceTimes = [];
    var pulses = [];
    var model = null;
    var selectedHash = null;
    var frame = 0;

    function el(id) { return typeof document !== 'undefined' ? document.getElementById(id) : null; }

    function records() {
        return (typeof _cachedConfigByName !== 'undefined' && _cachedConfigByName) || {};
    }

    function mergedEntries() {
        var seen = Object.create(null);
        var out = [];
        paths.forEach(function(p) {
            var hash = String(p.hash || p.destination_hash || '');
            if (!hash || seen[hash]) return;
            seen[hash] = true;
            out.push({ hash: hash, hops: p.hops, iface: p.interface || UNKNOWN, ts: p.timestamp || 0 });
        });
        var now = Date.now();
        Object.keys(recent).forEach(function(hash) {
            var r = recent[hash];
            if (now - r.at > FRESH_MS) { delete recent[hash]; return; }
            if (seen[hash]) return;
            out.push({ hash: hash, hops: r.hops, iface: r.iface || UNKNOWN, ts: r.at / 1000, fresh: true });
        });
        return out;
    }

    function reducedMotion() {
        return typeof window !== 'undefined' && window.matchMedia &&
            window.matchMedia('(prefers-reduced-motion: reduce)').matches;
    }

    function cssVar(name, fallback) {
        try {
            var value = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
            return value || fallback;
        } catch (_) { return fallback; }
    }

    function sizeCanvas(canvas) {
        var rect = canvas.getBoundingClientRect();
        var width = Math.max(200, Math.round(rect.width));
        var height = Math.max(180, Math.round(rect.height));
        var ratio = Math.min(window.devicePixelRatio || 1, 2);
        if (canvas.width !== Math.round(width * ratio) || canvas.height !== Math.round(height * ratio)) {
            canvas.width = Math.round(width * ratio);
            canvas.height = Math.round(height * ratio);
        }
        return { width: width, height: height, ratio: ratio };
    }

    function hexToRgba(hex, alpha) {
        var n = parseInt(hex.slice(1), 16);
        return 'rgba(' + (n >> 16 & 255) + ',' + (n >> 8 & 255) + ',' + (n & 255) + ',' + alpha + ')';
    }

    function draw(now) {
        frame = 0;
        var canvas = el('announce-radar-canvas');
        if (!canvas || !canvas.offsetParent) return;
        var size = sizeCanvas(canvas);
        var ctx = canvas.getContext('2d');
        ctx.setTransform(size.ratio, 0, 0, size.ratio, 0, 0);
        ctx.clearRect(0, 0, size.width, size.height);

        model = layout(mergedEntries(), size.width, size.height, records());
        var line = cssVar('--border-surface', 'rgba(128,128,128,0.35)');
        var muted = cssVar('--text-muted', '#8b949e');
        var primary = cssVar('--text-primary', '#e6edf3');

        // Interface sectors (very light wedges) and hop rings.
        if (model.sectors.length > 1) {
            model.sectors.forEach(function(s) {
                ctx.beginPath();
                ctx.moveTo(model.cx, model.cy);
                ctx.arc(model.cx, model.cy, model.radius, s.start, s.start + s.size);
                ctx.closePath();
                ctx.fillStyle = hexToRgba(s.color, 0.07);
                ctx.fill();
            });
        }
        ctx.lineWidth = 1;
        ctx.strokeStyle = line;
        ctx.fillStyle = muted;
        ctx.font = '10px sans-serif';
        for (var h = 1; h <= model.maxHops; h++) {
            ctx.beginPath();
            ctx.arc(model.cx, model.cy, h * model.gap, 0, 2 * Math.PI);
            ctx.stroke();
            ctx.fillText(h + 'h', model.cx + 3, model.cy - h * model.gap + 10);
        }

        // Destinations.
        var byHash = Object.create(null);
        model.points.forEach(function(p) {
            byHash[p.hash] = p;
            ctx.beginPath();
            ctx.arc(p.x, p.y, p.hash === selectedHash ? 5 : 3, 0, 2 * Math.PI);
            ctx.fillStyle = hexToRgba(p.color, p.fresh ? 1 : 0.8);
            ctx.fill();
            if (p.hash === selectedHash) {
                ctx.strokeStyle = primary;
                ctx.lineWidth = 1.5;
                ctx.stroke();
                ctx.lineWidth = 1;
            }
        });

        // Waves and travelling pulses.
        var alive = [];
        var t = now || Date.now();
        pulses.forEach(function(pulse) {
            var state = pulseState(t - pulse.born);
            var p = byHash[pulse.hash];
            if (!state) return;
            alive.push(pulse);
            if (!p) return;
            if (state.wave !== null) {
                [0, 0.3, 0.6].forEach(function(delay) {
                    var k = state.wave - delay;
                    if (k <= 0 || k >= 1) return;
                    ctx.beginPath();
                    ctx.arc(p.x, p.y, 4 + k * 34, 0, 2 * Math.PI);
                    ctx.strokeStyle = hexToRgba(p.color, 0.65 * (1 - k));
                    ctx.lineWidth = 2;
                    ctx.stroke();
                });
                ctx.lineWidth = 1;
            }
            if (state.travel !== null) {
                var k2 = state.travel;
                var x = p.x + (model.cx - p.x) * k2;
                var y = p.y + (model.cy - p.y) * k2;
                ctx.beginPath();
                ctx.arc(x, y, 3.5, 0, 2 * Math.PI);
                ctx.fillStyle = hexToRgba(p.color, 1 - 0.4 * k2);
                ctx.fill();
            }
        });
        pulses = alive;

        // This node.
        ctx.beginPath();
        ctx.arc(model.cx, model.cy, 5, 0, 2 * Math.PI);
        ctx.fillStyle = primary;
        ctx.fill();

        renderMeta();
        if (pulses.length && !reducedMotion()) schedule();
        else if (pulses.length) setTimeout(schedule, 400);
    }

    function schedule() {
        if (frame) return;
        var raf = window.requestAnimationFrame || function(cb) { return setTimeout(function() { cb(Date.now()); }, 33); };
        frame = raf(draw);
    }

    function renderMeta() {
        var summary = el('announce-radar-summary');
        if (summary && model) summary.textContent = String(model.points.length);
        var legend = el('announce-radar-legend');
        if (legend && model) {
            legend.innerHTML = model.sectors.map(function(s) {
                return '<span class="announce-radar-legend-item"><span class="announce-radar-dot" style="background:' +
                    s.color + '"></span>' + escapeHtml(s.name) + ' <span class="announce-radar-count">' + s.count + '</span></span>';
            }).join('');
        }
        var now = Date.now();
        announceTimes = announceTimes.filter(function(ts) { return now - ts < 60000; });
        var rate = el('announce-radar-rate');
        if (rate) rate.textContent = String(announceTimes.length);
    }

    // ---- Public hooks (called from health.js) -------------------------------

    function announceRadarUpdate(pathTable) {
        paths = Array.isArray(pathTable) ? pathTable : [];
        schedule();
    }

    function announceRadarPulse(event) {
        if (!event || !event.hash) return;
        var hash = String(event.hash);
        var known = null;
        for (var i = 0; i < paths.length; i++) {
            if ((paths[i].hash || paths[i].destination_hash) === hash) { known = paths[i]; break; }
        }
        var now = Date.now();
        recent[hash] = { hops: event.hops, iface: known ? known.interface : null, at: now,
            name: event.display_name || '' };
        announceTimes.push(now);
        pulses.push({ hash: hash, born: now });
        if (pulses.length > MAX_PULSES) pulses.splice(0, pulses.length - MAX_PULSES);
        schedule();
    }

    function showTip(point) {
        var tip = el('announce-radar-tip');
        if (!tip) return;
        if (!point) { tip.textContent = tip.dataset.idle || ''; return; }
        var name = recent[point.hash] && recent[point.hash].name;
        tip.textContent = (name ? name + ' · ' : '') + point.hash.slice(0, 8) + '… · ' + point.hops + 'h · ' + point.iface;
    }

    function bind() {
        var card = el('announce-radar-card');
        var canvas = el('announce-radar-canvas');
        if (!card || !canvas) return;
        var header = card.querySelector('.announce-radar-header');
        function toggle() {
            var collapsed = card.classList.toggle('collapsed');
            header.setAttribute('aria-expanded', collapsed ? 'false' : 'true');
            if (!collapsed) schedule();
        }
        header.addEventListener('click', toggle);
        header.addEventListener('keydown', function(e) {
            if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); toggle(); }
        });
        var tip = el('announce-radar-tip');
        if (tip) tip.dataset.idle = tip.textContent;
        canvas.addEventListener('pointerdown', function(e) {
            var rect = canvas.getBoundingClientRect();
            var point = nearestPoint(model, e.clientX - rect.left, e.clientY - rect.top, 16);
            selectedHash = point ? point.hash : null;
            showTip(point);
            schedule();
        });
        if (typeof ResizeObserver === 'function') new ResizeObserver(schedule).observe(canvas);
        document.addEventListener('visibilitychange', function() { if (!document.hidden) schedule(); });
        schedule();
    }

    window.announceRadarUpdate = announceRadarUpdate;
    window.announceRadarPulse = announceRadarPulse;
    window.RS = window.RS || {};
    RS.announceRadar = { classify: classify, layout: layout, nearestPoint: nearestPoint, pulseState: pulseState,
        fractions: fractions, MAX_POINTS: MAX_POINTS };
    if (typeof document !== 'undefined' && document.addEventListener) {
        if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', bind);
        else bind();
    }
})();
