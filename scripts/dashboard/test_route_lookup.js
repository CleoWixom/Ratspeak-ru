#!/usr/bin/env node
// Route lookup: reply normalisation and the find/refresh state machine.
'use strict';

var assert = require('assert');
var fs = require('fs');
var path = require('path');
var vm = require('vm');

var source = fs.readFileSync(path.join(__dirname, '..', '..', 'dashboard', 'static', 'js', 'route_lookup.js'), 'utf8');
var window = {};
var context = { window: window, RS: {}, isFinite: isFinite, Number: Number, Math: Math, String: String, Promise: Promise };
window.RS = context.RS;
vm.runInNewContext(source, context, { filename: 'route_lookup.js' });
var api = context.RS.routeLookup;
assert(api && typeof window.openRouteSheet === 'function');
var plain = function(v) { return JSON.parse(JSON.stringify(v)); };

var HASH = 'ab'.repeat(16);

// Addresses.
assert(api.validHash(HASH));
['', 'zz', HASH.slice(1), HASH + 'a', null, undefined, 12].forEach(function(bad) { assert(!api.validHash(bad)); });

// Time left: absolute epoch or seconds remaining.
assert.strictEqual(api.secondsLeft(1700000600, 1700000000), 600);
assert.strictEqual(api.secondsLeft(1699999000, 1700000000), 0);
assert.strictEqual(api.secondsLeft(90, 1700000000), 90);
['', null, 0, -5, 'x', NaN].forEach(function(bad) { assert.strictEqual(api.secondsLeft(bad, 1), null); });
assert.strictEqual(api.formatDuration(45), '45s');
assert.strictEqual(api.formatDuration(60), '1m');
assert.strictEqual(api.formatDuration(125), '2m 5s');
assert.strictEqual(api.formatDuration(3600), '1h');
assert.strictEqual(api.formatDuration(5400), '1h 30m');
assert.strictEqual(api.formatDuration(null), '');

// Replies.
assert.deepStrictEqual(plain(api.normalize({ has_path: false }, 0)), { known: false, error: '' });
assert.deepStrictEqual(plain(api.normalize({ has_path: false, error: 'RNS not initialized' }, 0)),
    { known: false, error: 'RNS not initialized' });
assert.deepStrictEqual(plain(api.normalize(null, 0)), { known: false, error: '' });
assert.deepStrictEqual(plain(api.normalize({ has_path: true, hops: 3, interface: 'LoRa', via: 'cd'.repeat(16), expires: 1700000300 }, 1700000000)),
    { known: true, hops: 3, iface: 'LoRa', via: 'cd'.repeat(16), left: 300 });
var direct = plain(api.normalize({ has_path: true, hops: 1, interface: 'Hub', via: null, expires: 0 }, 5));
assert.strictEqual(direct.via, '');
assert.strictEqual(direct.left, null);
assert.strictEqual(plain(api.normalize({ has_path: true, hops: 'x' }, 0)).hops, null);

// A fake clock and transport.
function harness(script) {
    var t = 0, calls = [], states = [];
    var deps = {
        now: function() { return t; },
        sleep: function(ms) { t += ms; return Promise.resolve(); },
        onState: function(s) { states.push(s.phase + ':' + s.route.known); },
        invoke: function(command, args) {
            calls.push(command);
            if (command === 'request_path') return Promise.resolve(script.request ? script.request(t, calls) : { success: true });
            return Promise.resolve(script.query(t));
        }
    };
    return { deps: deps, calls: calls, states: states };
}
var known = { has_path: true, hops: 2, interface: 'Hub', via: null, expires: 120 };

async function main() {
    // No path yet, found after a few polls.
    var h = harness({ query: function(t) { return t >= 3000 ? known : { has_path: false }; } });
    var found = plain(await api.findRoute(HASH, h.deps));
    assert.strictEqual(found.phase, 'found');
    assert.strictEqual(found.route.hops, 2);
    assert.strictEqual(h.calls.filter(function(c) { return c === 'request_path'; }).length, 1);

    // Never found: request is repeated once, then it gives up.
    h = harness({ query: function() { return { has_path: false }; } });
    var none = plain(await api.findRoute(HASH, h.deps));
    assert.strictEqual(none.phase, 'none');
    assert.strictEqual(h.calls.filter(function(c) { return c === 'request_path'; }).length, 2);
    assert(h.states.length >= 10);

    // Path already known: refresh waits, then reports the current path.
    h = harness({ query: function() { return known; } });
    var refreshed = plain(await api.findRoute(HASH, h.deps));
    assert.strictEqual(refreshed.phase, 'found');
    assert.strictEqual(h.calls.filter(function(c) { return c === 'request_path'; }).length, 1);

    // Path disappears while refreshing.
    h = harness({ query: function(t) { return t === 0 ? known : { has_path: false }; } });
    assert.strictEqual(plain(await api.findRoute(HASH, h.deps)).phase, 'lost');

    // Transport refused the request.
    h = harness({ query: function() { return { has_path: false }; }, request: function() { return { success: false }; } });
    assert.strictEqual(plain(await api.findRoute(HASH, h.deps)).phase, 'unavailable');

    // A failing query counts as unknown, not a crash.
    h = harness({ query: function() { throw new Error('boom'); } });
    h.deps.invoke = function(command) {
        if (command === 'request_path') return Promise.resolve({ success: true });
        return Promise.reject(new Error('boom'));
    };
    assert.strictEqual(plain(await api.findRoute(HASH, h.deps)).phase, 'none');
    console.log('route lookup passed');
}
main().catch(function(error) { console.error(error); process.exit(1); });
