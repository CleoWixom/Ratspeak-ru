#!/usr/bin/env node
// The native connection-link presenter must only pre-fill the existing sheets:
// it validates the typed target, never submits, and rejects secret-shaped fields.
'use strict';

var assert = require('assert');
var fs = require('fs');
var path = require('path');
var vm = require('vm');
var source = fs.readFileSync(path.join(__dirname, '..', '..', 'dashboard', 'static', 'js', 'modals.js'), 'utf8');
var start = source.indexOf('function connectionsOpenNativeLink(target)');
var end = source.indexOf('window.connectionsOpenNativeLink = connectionsOpenNativeLink;');
assert(start > 0 && end > start, 'presenter present in modals.js');
var presenter = source.slice(start, end);

function sandbox() {
    var els = {};
    ['connect-host', 'connect-port', 'connect-name', 'rnode-iface-name'].forEach(function(id) {
        els[id] = { value: '' };
    });
    els['connect-name-field'] = { style: { display: 'none' } };
    var s = { calls: [], toasts: [], applied: [], catalogHandlers: [], els: els };
    var ctx = {
        document: { getElementById: function(id) { return els[id] || null; } },
        openConnectModal: function() { s.calls.push('openConnectModal'); },
        setConnectTab: function(tab) { s.calls.push('setConnectTab:' + tab); els['connect-name'].value = ''; },
        openRnodeModal: function(mode) { s.calls.push('openRnodeModal:' + mode); },
        loadRnodePresetCatalog: function() {
            return { then: function(ok, fail) { s.catalogHandlers.push([ok, fail]); } };
        },
        _rnodeRegionForFrequency: function() { return 'custom'; },
        _rnodePresetForParams: function() { return 'custom'; },
        _rnodeApplyRadioControls: function() { s.applied.push([].slice.call(arguments)); },
        showToast: function(message, cls) { s.toasts.push([message, cls]); },
        // Anything that would add/connect must never be reached.
        submitConnection: function() { throw new Error('presenter must not submit'); },
        rnodeWizardNext: function() { throw new Error('presenter must not advance'); },
        RS: { invoke: function() { throw new Error('presenter must not call IPC'); } },
        Object: Object, Number: Number, String: String
    };
    vm.runInNewContext(presenter, ctx, { filename: 'presenter' });
    s.open = ctx.connectionsOpenNativeLink;
    return s;
}

function tcp(extra) {
    return Object.assign({ format: 'ratspeak.connection.v1', kind: 'tcp',
        payload: 'ratspeak://connection?v=1&kind=tcp&host=rns.example.org&port=4242&name=Hub',
        name: 'Hub', host: 'rns.example.org', port: 4242, frequency: null, bandwidth: null,
        spreading_factor: null, coding_rate: null, tx_power: null }, extra || {});
}
function radio(extra) {
    return Object.assign({ format: 'ratspeak.connection.v1', kind: 'radio',
        payload: 'ratspeak://connection?v=1&kind=radio&freq=868000000&bw=125000&sf=8&cr=5&tx=14',
        name: null, host: null, port: null, frequency: 868000000, bandwidth: 125000,
        spreading_factor: 8, coding_rate: 5, tx_power: 14 }, extra || {});
}

// TCP: opens the custom tab, fills host/port/name (after the tab clears the name), no submit.
var s = sandbox();
assert.strictEqual(s.open(tcp()), true);
assert.deepStrictEqual(s.calls, ['openConnectModal', 'setConnectTab:custom']);
assert.strictEqual(s.els['connect-host'].value, 'rns.example.org');
assert.strictEqual(s.els['connect-port'].value, '4242');
assert.strictEqual(s.els['connect-name'].value, 'Hub');
assert.strictEqual(s.els['connect-name-field'].style.display, '');
assert.strictEqual(s.toasts.length, 1);

s = sandbox();
assert.strictEqual(s.open(tcp({ name: null, payload: 'ratspeak://connection?v=1&kind=tcp&host=a.example&port=1' })), true);
assert.strictEqual(s.els['connect-name-field'].style.display, 'none');

// Radio: opens at step 1 (device is chosen locally), applies values now and after the catalog loads.
s = sandbox();
assert.strictEqual(s.open(radio({ name: 'City' })), true);
assert.deepStrictEqual(s.calls, ['openRnodeModal:ble']);
assert.deepStrictEqual(s.applied[0].slice(2), [868000000, 125000, 8, 5, 14, true]);
assert.strictEqual(s.els['rnode-iface-name'].value, 'City');
assert.strictEqual(s.catalogHandlers.length, 1);
s.catalogHandlers[0][0]();
s.catalogHandlers[0][1]();
assert.strictEqual(s.applied.length, 3, 're-applied after catalog success and failure');

// Rejections: nothing is opened for malformed or secret-shaped targets.
var bad = [
    null, {}, tcp({ format: 'ratspeak.channel.v1' }), tcp({ kind: 'ble' }),
    tcp({ payload: 'x'.repeat(231) }), tcp({ host: 'a/b' }), tcp({ host: '' }),
    tcp({ host: 'a b' }), tcp({ host: 5 }), tcp({ port: 0 }), tcp({ port: 65536 }),
    tcp({ port: '4242' }), tcp({ port: 1.5 }), tcp({ name: '' }), tcp({ name: 'x'.repeat(33) }),
    tcp({ name: 'a\nb' }), tcp({ name: 7 }),
    tcp({ ifac: 'x' }), tcp({ passphrase: 'x' }), tcp({ key: 'x' }), tcp({ join_key: 'x' }),
    tcp({ ifac_passphrase: 'x' }), tcp({ device: 'ble://AA' }), radio({ address: 'AA:BB' }),
    radio({ frequency: 136999999 }), radio({ frequency: 3000000001 }), radio({ frequency: 868.5 }),
    radio({ bandwidth: 7799 }), radio({ bandwidth: 1625001 }), radio({ spreading_factor: 4 }),
    radio({ spreading_factor: 13 }), radio({ coding_rate: 4 }), radio({ coding_rate: 9 }),
    radio({ tx_power: -1 }), radio({ tx_power: 38 }), radio({ tx_power: null })
];
bad.forEach(function(target, index) {
    var sb = sandbox();
    assert.strictEqual(sb.open(target), false, 'rejected #' + index);
    assert.deepStrictEqual(sb.calls, [], 'no sheet opened for #' + index);
    assert.strictEqual(sb.toasts.length, 0);
});

console.log('connection link presenter tests passed (tcp, radio, rejection matrix, no submit)');
