#!/usr/bin/env node
// Announce propagation radar: geometry, classification and hit testing.
'use strict';

var assert = require('assert');
var fs = require('fs');
var path = require('path');
var vm = require('vm');

var source = fs.readFileSync(path.join(__dirname, '..', '..', 'dashboard', 'static', 'js', 'announce_radar.js'), 'utf8');
var window = {};
var context = { window: window, RS: {}, parseInt: parseInt, Math: Math, Object: Object, String: String,
    Array: Array, Date: Date, setTimeout: setTimeout };
window.RS = context.RS;
vm.runInNewContext(source, context, { filename: 'announce_radar.js' });
var radar = context.RS.announceRadar;
assert(radar && typeof window.announceRadarUpdate === 'function' && typeof window.announceRadarPulse === 'function');
var plain = function(v) { return JSON.parse(JSON.stringify(v)); };

// Interface classification.
assert.strictEqual(radar.classify('Radio', { type: 'rnode' }), 'lora');
assert.strictEqual(radar.classify('LoRa 868'), 'lora');
assert.strictEqual(radar.classify('Hub', { type: 'tcp_client' }), 'tcp');
assert.strictEqual(radar.classify('Backbone 1'), 'tcp');
assert.strictEqual(radar.classify('BLE peer'), 'ble');
assert.strictEqual(radar.classify('Auto discovery', { type: 'auto' }), 'local');
assert.strictEqual(radar.classify('Something'), 'other');

// Stable fractions in [0, 1).
var f = radar.fractions('0123456789abcdef0123456789abcdef');
assert.deepStrictEqual(plain(f), plain(radar.fractions('0123456789abcdef0123456789abcdef')));
f.forEach(function(v) { assert(v >= 0 && v < 1); });
radar.fractions('').forEach(function(v) { assert.strictEqual(v, 0); });

// Layout: rings for hops, one sector per interface, points inside their sector.
function hash(n) { return (n.toString(16) + 'f3a9c2d1e7b8a4560123456789abcdef').slice(0, 32).padStart(32, 'a'); }
var entries = [];
for (var i = 0; i < 60; i++) entries.push({ hash: hash(i * 7919 + 1), hops: 1 + (i % 5), iface: i % 3 ? 'Hub' : 'Radio', ts: 1000 + i });
entries.push({ hash: hash(99999), hops: 4, iface: 'Hub', ts: 5000, fresh: true });
var model = radar.layout(entries, 400, 300, { Radio: { type: 'rnode' }, Hub: { type: 'tcp_client' } });
assert.strictEqual(model.cx, 200);
assert.strictEqual(model.cy, 150);
assert.strictEqual(model.radius, 150 - 18);
assert.strictEqual(model.maxHops, 5);
assert.strictEqual(model.points.length, 61);
assert.strictEqual(model.sectors.length, 2);
assert.deepStrictEqual(model.sectors.map(function(s) { return s.name; }), ['Hub', 'Radio']);
assert.strictEqual(model.sectors[0].count + model.sectors[1].count, 61);
var sectorsByName = {};
model.sectors.forEach(function(s) { sectorsByName[s.name] = s; });
model.points.forEach(function(p) {
    var s = sectorsByName[p.iface];
    var rel = (p.angle - s.start) % (2 * Math.PI);
    assert(rel >= 0 && rel <= s.size, 'point stays inside its interface sector');
    assert(p.r > 0 && p.r <= model.radius + 1e-9, 'point stays inside the outer ring');
    assert(Math.abs(Math.hypot(p.x - model.cx, p.y - model.cy) - p.r) < 1e-6);
    assert(p.hops >= 1 && p.hops <= model.maxHops);
});
assert.strictEqual(model.sectors.find(function(s) { return s.name === 'Radio'; }).color, '#f0883e');
assert.strictEqual(model.sectors.find(function(s) { return s.name === 'Hub'; }).color, '#58a6ff');
assert.strictEqual(model.points.filter(function(p) { return p.fresh; }).length, 1);
// Same input, same picture.
assert.deepStrictEqual(plain(radar.layout(entries, 400, 300, {})).points.map(function(p) { return [p.x, p.y]; }),
    plain(model).points.map(function(p) { return [p.x, p.y]; }));

// Hop rings grow with hop count; deep hops are capped.
var deep = radar.layout([{ hash: hash(1), hops: 50, iface: 'A', ts: 1 }, { hash: hash(2), hops: 1, iface: 'A', ts: 2 }], 300, 300);
assert.strictEqual(deep.maxHops, 10);
var near = deep.points.find(function(p) { return p.hash === hash(2); });
var far = deep.points.find(function(p) { return p.hash === hash(1); });
assert(far.r > near.r);

// Empty and unknown interfaces.
var empty = radar.layout([], 300, 300);
assert.strictEqual(empty.points.length, 0);
assert.strictEqual(empty.maxHops, 3);
var unknown = radar.layout([{ hash: hash(3), hops: 2, ts: 1 }], 300, 300);
assert.strictEqual(unknown.points[0].iface, 'Unknown');
assert.strictEqual(unknown.sectors.length, 1);
assert.strictEqual(radar.layout([null, {}, { hash: '' }], 300, 300).points.length, 0);

// Only the newest destinations are drawn.
var many = [];
for (var j = 0; j < radar.MAX_POINTS + 50; j++) many.push({ hash: hash(j + 1000), hops: 2, iface: 'A', ts: j });
var capped = radar.layout(many, 300, 300);
assert.strictEqual(capped.points.length, radar.MAX_POINTS);
assert(capped.points.every(function(p) { return many.find(function(e) { return e.hash === p.hash; }).ts >= 50; }));

// Hit testing.
var target = model.points[7];
assert.strictEqual(radar.nearestPoint(model, target.x + 2, target.y - 2, 16).hash, target.hash);
assert.strictEqual(radar.nearestPoint(model, -500, -500, 16), null);
assert.strictEqual(radar.nearestPoint(null, 0, 0, 16), null);

// Pulse timeline.
assert.strictEqual(radar.pulseState(-1), null);
assert.deepStrictEqual(plain(radar.pulseState(0)), { wave: 0, travel: 0 });
var mid = radar.pulseState(900);
assert(mid.wave > 0 && mid.wave < 1 && mid.travel > 0 && mid.travel < 1);
var late = radar.pulseState(1500);
assert.strictEqual(late.travel, null);
assert(late.wave < 1);
assert.strictEqual(radar.pulseState(5000), null);

// Hooks tolerate junk and a page without the radar.
window.announceRadarUpdate('nope');
window.announceRadarUpdate([{ hash: hash(1), hops: 1, interface: 'Hub', timestamp: 1 }]);
window.announceRadarPulse(null);
window.announceRadarPulse({ hash: hash(1), hops: 1 });
console.log('announce radar passed');
