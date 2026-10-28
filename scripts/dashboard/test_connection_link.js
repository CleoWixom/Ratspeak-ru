#!/usr/bin/env node
// Coverage for ratspeak://tcp and ratspeak://radio: the acknowledged native
// bridge and the presenters that pre-fill the add-connection forms.
'use strict';

var assert = require('assert');
var fs = require('fs');
var path = require('path');
var vm = require('vm');

var jsDir = path.join(__dirname, '..', '..', 'dashboard', 'static', 'js');
var bridgeSource = fs.readFileSync(path.join(jsDir, 'native_connection_link.js'), 'utf8');
var presentersSource = fs.readFileSync(path.join(jsDir, 'connection_link_presenters.js'), 'utf8');

var tcp = { kind: 'tcp', format: 'ratspeak.tcp.v1', payload: 'ratspeak://tcp?v=1&host=a.example',
    host: 'a.example', port: 4242, name: null, ifac_network_name: null, ifac_passphrase: null };
var radio = { kind: 'radio', format: 'ratspeak.radio.v1', payload: 'ratspeak://radio?v=1',
    name: 'Local mesh', mode: 'gateway', region_key: 'europe', preset_key: 'medium_fast',
    frequency: 868000000, bandwidth: 250000, spreading_factor: 9, coding_rate: 5, tx_power: 14,
    custom_params: false, airtime_limit_short: null, airtime_limit_long: 10 };

// ---- Bridge ---------------------------------------------------------------

function bridgePage(native) {
    var state = { timers: [], presented: [], invokes: [], listener: null, observer: null,
        dialog: false, listenName: null };
    var document = {
        visibilityState: 'visible', body: {},
        addEventListener: function() {},
        querySelector: function() { return state.dialog ? {} : null; }
    };
    var rs = {
        diag: function() {},
        listen: function(name, handler, settings) {
            state.listenName = name;
            assert.strictEqual(settings.required, true);
            state.listener = handler;
            return Promise.resolve(function() {});
        },
        invoke: function(command, args) {
            state.invokes.push(command);
            if (command === 'peek_native_connection_link') return Promise.resolve(native.pending);
            assert.strictEqual(command, 'ack_native_connection_link');
            var matched = !!native.pending && native.pending.revision === args.revision &&
                native.pending.activityGeneration === args.activityGeneration;
            if (matched) native.pending = null;
            return Promise.resolve(matched);
        }
    };
    vm.runInNewContext(bridgeSource, {
        window: { __RATSPEAK_DESKTOP__: true, RS: rs,
            openNativeConnectionLink: function(value) {
                state.presented.push(value);
                state.dialog = true; // the real form opens a blocking sheet
                return true;
            } },
        RS: rs, document: document, _isSetupActive: function() { return false; },
        MutationObserver: function(callback) { state.observer = callback; this.observe = function() {}; },
        setTimeout: function(callback) { state.timers.push(callback); return state.timers.length; }
    }, { filename: 'native-connection-link.js' });
    state.settle = async function() {
        for (var round = 0; round < 30; round++) {
            for (var i = 0; i < 4; i++) await Promise.resolve();
            state.timers.splice(0).forEach(function(callback) { callback(); });
        }
        for (var flush = 0; flush < 8; flush++) await Promise.resolve();
        assert.strictEqual(state.timers.length, 0, 'bridge must not spin');
    };
    state.dismiss = function() { state.dialog = false; state.observer(); };
    return state;
}

async function testBridge() {
    var native = { pending: { revision: '1', activityGeneration: '1', target: tcp } };
    var page = bridgePage(native);
    await page.settle();
    assert.strictEqual(page.listenName, 'native_connection_link_available');
    assert.strictEqual(page.presented[0], tcp);
    assert.strictEqual(native.pending, null, 'presented link is acknowledged');
    assert(page.invokes.includes('ack_native_connection_link'));

    // A newer link waits while the form is open, then is presented once.
    native.pending = { revision: '2', activityGeneration: '1', target: radio };
    page.listener(); await page.settle();
    assert.strictEqual(page.presented.length, 1, 'open form must not be replaced');
    assert.strictEqual(native.pending.target, radio);
    page.dismiss(); await page.settle();
    assert.strictEqual(page.presented[1], radio);
    assert.strictEqual(native.pending, null);
}

// ---- Presenters -----------------------------------------------------------

function field(initial) { return { value: initial || '', checked: false, style: {} }; }

function presenterContext() {
    var ids = ['connect-host', 'connect-port', 'connect-name', 'connect-name-field',
        'connect-use-ifac', 'connect-ifac-network-name', 'connect-ifac-passphrase',
        'connect-ifac-size', 'rnode-iface-name'];
    var elements = {};
    ids.forEach(function(id) { elements[id] = field(); });
    var log = { calls: [], toasts: [], tabs: [], radio: null, airtime: null, mode: null,
        ifacSynced: 0 };
    var context = {
        document: { getElementById: function(id) { return elements[id] || null; } },
        window: {},
        showToast: function(message, kind) { log.toasts.push([message, kind]); },
        openConnectModal: function() { log.calls.push('openConnectModal'); },
        setConnectTab: function(tab) {
            log.tabs.push(tab);
            if (tab === 'custom') elements['connect-name'].value = ''; // real behaviour
        },
        _ifacSyncFields: function(prefix) { assert.strictEqual(prefix, 'connect'); log.ifacSynced++; },
        openRnodeModal: function(mode) { log.calls.push('openRnodeModal:' + mode); },
        loadRnodePresetCatalog: function() { return Promise.resolve({}); },
        _RNODE_CUSTOM_REGION_KEY: 'custom', _RNODE_CUSTOM_PRESET_KEY: 'custom',
        _rnodeApplyRadioControls: function() { log.radio = Array.prototype.slice.call(arguments); },
        _rnodeSetAirtimeLimits: function(a, b) { log.airtime = [a, b]; },
        _rnodeSetInterfaceMode: function(mode) { log.mode = mode; }
    };
    vm.runInNewContext(presentersSource, context, { filename: 'connection_link_presenters.js' });
    return { context: context, elements: elements, log: log };
}

async function testTcpPresenter() {
    var page = presenterContext();
    var open = page.context.window.openNativeConnectionLink;
    assert.strictEqual(typeof open, 'function');

    // Host/port only: no IFAC, name field stays hidden, nothing auto-submits.
    assert.strictEqual(open(tcp), true);
    assert.deepStrictEqual(page.log.calls, ['openConnectModal']);
    assert.deepStrictEqual(page.log.tabs, ['custom']);
    assert.strictEqual(page.elements['connect-host'].value, 'a.example');
    assert.strictEqual(page.elements['connect-port'].value, '4242');
    assert.strictEqual(page.elements['connect-use-ifac'].checked, false);
    assert.strictEqual(page.elements['connect-name-field'].style.display, 'none');
    assert(page.log.toasts.length === 1);

    // With IFAC and a name (name must survive the tab switch).
    page = presenterContext();
    open = page.context.window.openNativeConnectionLink;
    assert.strictEqual(open({ kind: 'tcp', host: '2001:db8::1', port: 7000, name: 'My node',
        ifac_network_name: 'mynet', ifac_passphrase: 'secret' }), true);
    assert.strictEqual(page.elements['connect-name'].value, 'My node');
    assert.strictEqual(page.elements['connect-name-field'].style.display, '');
    assert.strictEqual(page.elements['connect-use-ifac'].checked, true);
    assert.strictEqual(page.elements['connect-ifac-network-name'].value, 'mynet');
    assert.strictEqual(page.elements['connect-ifac-passphrase'].value, 'secret');
    assert.strictEqual(page.elements['connect-ifac-size'].value, '');

    // Only one IFAC field is still IFAC.
    page = presenterContext();
    open = page.context.window.openNativeConnectionLink;
    assert.strictEqual(open({ kind: 'tcp', host: 'a.example', port: 1, ifac_passphrase: 'only' }), true);
    assert.strictEqual(page.elements['connect-use-ifac'].checked, true);
    assert.strictEqual(page.elements['connect-ifac-network-name'].value, '');

    // Malformed typed targets are rejected without touching the UI.
    page = presenterContext();
    open = page.context.window.openNativeConnectionLink;
    [null, 'x', {}, { kind: 'tcp' }, { kind: 'tcp', host: '', port: 4242 },
        { kind: 'tcp', host: 'a', port: 0 }, { kind: 'tcp', host: 'a', port: 65536 },
        { kind: 'tcp', host: 'a', port: '4242' }, { kind: 'other' }].forEach(function(bad) {
        assert.strictEqual(open(bad), false);
    });
    assert.deepStrictEqual(page.log.calls, []);
}

async function testRadioPresenter() {
    var page = presenterContext();
    var open = page.context.window.openNativeConnectionLink;
    assert.strictEqual(open(radio), true);
    assert.deepStrictEqual(page.log.calls, ['openRnodeModal:ble']);
    await Promise.resolve(); await Promise.resolve();
    assert.deepStrictEqual(page.log.radio,
        ['europe', 'medium_fast', 868000000, 250000, 9, 5, 14, true]);
    assert.deepStrictEqual(page.log.airtime, [null, 10]);
    assert.strictEqual(page.log.mode, 'gateway');
    assert.strictEqual(page.elements['rnode-iface-name'].value, 'Local mesh');

    // Custom parameters use the custom region/preset keys.
    page = presenterContext();
    open = page.context.window.openNativeConnectionLink;
    assert.strictEqual(open({ kind: 'radio', name: null, mode: null, region_key: null,
        preset_key: null, frequency: 433000000, bandwidth: 125000, spreading_factor: 7,
        coding_rate: 5, tx_power: 10, custom_params: true, airtime_limit_short: null,
        airtime_limit_long: null }), true);
    await Promise.resolve(); await Promise.resolve();
    assert.deepStrictEqual(page.log.radio,
        ['custom', 'custom', 433000000, 125000, 7, 5, 10, true]);
    assert.strictEqual(page.log.mode, null);
    assert.strictEqual(page.elements['rnode-iface-name'].value, '');

    // Named link without a plain advanced section.
    page = presenterContext();
    open = page.context.window.openNativeConnectionLink;
    var plain = Object.assign({}, radio, { airtime_limit_long: null, name: null, mode: null });
    assert.strictEqual(open(plain), true);
    await Promise.resolve(); await Promise.resolve();
    assert.strictEqual(page.log.radio[7], false);

    // Out-of-range values never reach the UI.
    page = presenterContext();
    open = page.context.window.openNativeConnectionLink;
    [Object.assign({}, radio, { spreading_factor: 13 }),
        Object.assign({}, radio, { coding_rate: 4 }),
        Object.assign({}, radio, { tx_power: 38 }),
        Object.assign({}, radio, { frequency: '868000000' }),
        Object.assign({}, radio, { region_key: null })].forEach(function(bad) {
        assert.strictEqual(open(bad), false);
    });
    assert.deepStrictEqual(page.log.calls, []);
}

async function main() {
    await testBridge();
    await testTcpPresenter();
    await testRadioPresenter();
    console.log('connection link bridge and presenters passed');
}

main().catch(function(error) { console.error(error); process.exit(1); });
