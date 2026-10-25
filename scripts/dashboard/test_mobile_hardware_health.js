#!/usr/bin/env node
'use strict';

var assert = require('assert');
var fs = require('fs');
var path = require('path');
var vm = require('vm');

var source = fs.readFileSync(
    path.join(__dirname, '..', '..', 'dashboard', 'static', 'js', 'health.js'),
    'utf8'
);
var start = source.indexOf('var RNODE_STATE_LABELS');
var end = source.indexOf('\nfunction _interfaceConfigByName', start);
assert(start >= 0 && end > start, 'mobile hardware health reducer must exist');
var context = {
    isInterfaceConfigEnabled: function(iface) { return iface.enabled !== false; }
};
vm.runInNewContext(source.slice(start, end), context, { filename: 'mobile-hardware-health.js' });

assert.deepStrictEqual(
    JSON.parse(JSON.stringify(context._mobileRnodeHealth('androidusb://opaque', {
        usb_rnode: { state: 'permission_needed' }
    }))),
    {
        kind: 'usb_rnode', state: 'permission_needed', reason: '',
        label: 'USB permission needed', actionable: true
    }
);
assert.deepStrictEqual(
    JSON.parse(JSON.stringify(context._mobileRnodeHealth('ble://00:11:22:33:44:55', {
        ble_rnode: { state: 'failed', reason: 'bluetooth_off' }
    }))),
    {
        kind: 'ble_rnode', state: 'failed', reason: 'bluetooth_off',
        label: 'Bluetooth is off', actionable: true
    }
);
['waiting_for_radio', 'reconnecting'].forEach(function(state) {
    assert.strictEqual(
        context._mobileRnodeHealth('ble://00:11:22:33:44:55', {
            ble_rnode: { state: state }
        }).label,
        'Waiting for radio'
    );
});
assert.strictEqual(
    context._mobileRnodeHealth('ble://00:11:22:33:44:55', {
        ble_rnode: { state: 'initializing' }
    }).label,
    'Initializing'
);
// A retrying radio keeps explaining its last failure through the next attempt.
[
    ['not_reachable', 'Radio not found'],
    ['link_lost', 'Connection dropped'],
    ['service_missing', 'No RNode service'],
    ['permission_needed', 'Bluetooth permission needed'],
    ['radio_disconnected', null],
    ['unknown_reason', null],
].forEach(function(row) {
    ['waiting_for_radio', 'connecting'].forEach(function(state) {
        var health = context._mobileRnodeHealth('ble://00:11:22:33:44:55', {
            ble_rnode: { state: state, reason: row[0] }
        });
        var generic = state === 'connecting' ? 'Connecting' : 'Waiting for radio';
        assert.strictEqual(health.label, row[1] || generic, state + '/' + row[0]);
        assert.strictEqual(health.actionable, row[0] === 'permission_needed', state + '/' + row[0]);
    });
});
assert.strictEqual(
    context._mobileRnodeHealth('ble://00:11:22:33:44:55', {
        ble_rnode: { state: 'failed', reason: 'stale_bond' }
    }).label,
    'Pair again'
);
[
    ['rejected_tx_power', 'TX power not supported'],
    ['rejected_frequency', 'Frequency not supported'],
    ['unsupported_firmware', 'Firmware update needed'],
].forEach(function(row) {
    var health = context._mobileRnodeHealth('ble://00:11:22:33:44:55', {
        ble_rnode: { state: 'failed', reason: row[0] }
    });
    assert.strictEqual(health.label, row[1], row[0]);
    assert.strictEqual(health.actionable, true, row[0]);
});
assert.strictEqual(
    context._mobileRnodeHealth('ble://00:11:22:33:44:55', {
        ble_rnode: { state: 'connected' }
    }),
    null,
    'healthy BLE must not retain a warning pill'
);
// Desktop and Android USB rows read the driver's own state from live stats.
[
    [{ state: 'failed', reason: 'rejected_tx_power' }, 'TX power not supported', true],
    [{ state: 'failed', reason: 'unsupported_radio' }, 'Radio not supported', true],
    [{ state: 'waiting_for_radio', reason: 'not_reachable' }, 'Radio not found', false],
    [{ state: 'waiting_for_radio', reason: 'link_lost' }, 'Connection dropped', false],
    [{ state: 'waiting_for_radio', reason: 'port_not_found' }, 'Radio not found', false],
    [{ state: 'waiting_for_radio', reason: 'port_access_denied' }, 'Port access denied', false],
    [{ state: 'waiting_for_radio', reason: 'port_in_use' }, 'Port in use', false],
    [{ state: 'initializing', reason: null }, 'Initializing', false],
    [{ state: 'connecting', reason: null }, 'Connecting', false],
].forEach(function(row) {
    var health = context._rnodeRuntimeHealth(row[0]);
    assert.strictEqual(health.label, row[1], JSON.stringify(row[0]));
    assert.strictEqual(health.actionable, row[2], JSON.stringify(row[0]));
});
assert.strictEqual(context._rnodeRuntimeHealth(null), null, 'a Ready radio has no row state');
assert.deepStrictEqual(
    JSON.parse(JSON.stringify(context._configuredInterfaceFallback({
        ifaceType: 'rnode',
        iface: { port: '/dev/cu.usbserial-0001', enabled: true }
    }, {}))),
    {
        paused: false, waitingForDevice: false, mobileHealth: null, connecting: true
    },
    'a configured serial radio must render as connecting before live stats arrive'
);
assert.deepStrictEqual(
    JSON.parse(JSON.stringify(context._configuredInterfaceFallback({
        ifaceType: 'rnode',
        iface: { port: '/dev/cu.usbserial-0001', enabled: false }
    }, {}))),
    {
        paused: true, waitingForDevice: false, mobileHealth: null, connecting: false
    },
    'a paused configured radio must remain visible without live stats'
);
assert(source.includes('if (online) mobileHealth = null;'),
    'ambient USB/BLE state must never override exact online transport evidence');
assert(source.includes('function applyMobileHardwareState(data)'),
    'live native state must update the retained interface projection');

var actionStart = source.indexOf('function buildIfaceActionItems');
var actionEnd = source.indexOf('\nfunction setInterfacePaused', actionStart);
assert(actionStart >= 0 && actionEnd > actionStart, 'interface action reducer must exist');
var actionCalls = [];
var dialogs = [];
var androidCalls = [];
var actionRecord = {
    ifaceType: 'rnode',
    iface: { port: 'ble://00:11:22:33:44:55', enabled: true }
};
var actionContext = {
    ICON_PLAY: 'play', ICON_PAUSE: 'pause', ICON_RADIO: 'radio',
    ICON_PENCIL: 'pencil', ICON_TRASH: 'trash', ICON_INFO: 'info', ICON_QR: 'qr',
    openConnectionQrShare: function(type, name) { actionCalls.push(['qr', type, name]); },
    window: {
        RatspeakAndroid: {
            openBluetoothSettings: function() { androidCalls.push('bluetooth'); return true; },
            openAppPermissionSettings: function() { androidCalls.push('app'); return true; },
        }
    },
    hasAndroidBridge: function() { return true; },
    rsAlert: function(opts) { dialogs.push(['alert', opts.title]); return Promise.resolve(); },
    rsConfirm: function(opts) { dialogs.push(['confirm', opts.title]); return Promise.resolve(true); },
    _ensureBlePermissions: function() {
        actionCalls.push(['ble-permission']);
        return Promise.resolve(actionContext.permissionGranted);
    },
    permissionGranted: true,
    _cachedConfigIfaces: { mobile_hardware: {} },
    getConfiguredInterfaceRecord: function() { return actionRecord; },
    getInterfaceLiveStatus: function() { return actionContext.liveStatus; },
    liveStatus: null,
    lastStats: null,
    isInterfaceConfigEnabled: function(iface) { return iface.enabled !== false; },
    isLoraInterfaceType: function() { return true; },
    isTcpInterfaceType: function() { return false; },
    setInterfacePaused: function(type, name, paused) { actionCalls.push([type, name, paused]); },
    openInterfaceEdit: function(type, name) { actionCalls.push(['edit', type, name]); },
    openRenameInterfaceDialog: function() {},
    confirmRemoveInterface: function() {},
};
vm.runInNewContext(source.slice(actionStart, actionEnd), actionContext, {
    filename: 'interface-action-items.js'
});

['waiting_for_radio', 'reconnecting', 'connecting', 'initializing'].forEach(function(state) {
    actionContext._cachedConfigIfaces.mobile_hardware.ble_rnode = { state: state };
    var item = actionContext.buildIfaceActionItems('rnode', 'Radio')[0];
    assert.strictEqual(item.label, 'Connecting…');
    assert.strictEqual(item.disabled, true);
    assert.strictEqual(item.onSelect, undefined);
});

actionContext._cachedConfigIfaces.mobile_hardware.ble_rnode = { state: 'failed' };
var retry = actionContext.buildIfaceActionItems('rnode', 'Radio')[0];
assert.strictEqual(retry.label, 'Retry Connection');
retry.onSelect();
assert.deepStrictEqual(actionCalls.pop(), ['rnode', 'Radio', false]);

actionContext._cachedConfigIfaces.mobile_hardware.ble_rnode = { state: 'conflict' };
var conflict = actionContext.buildIfaceActionItems('rnode', 'Radio')[0];
assert.strictEqual(conflict.label, 'Radio conflict');
assert.strictEqual(conflict.disabled, true);

actionContext._cachedConfigIfaces.mobile_hardware.ble_rnode = { state: 'connected' };
var qrItem = actionContext.buildIfaceActionItems('rnode', 'Radio').filter(function(item) { return item.label === 'Share QR code'; })[0];
assert(qrItem, 'radio menu offers Share QR code');
qrItem.onSelect();
assert.deepStrictEqual(actionCalls.pop(), ['qr', 'rnode', 'Radio']);
var pause = actionContext.buildIfaceActionItems('rnode', 'Radio')[0];
assert.strictEqual(pause.label, 'Pause Interface');
pause.onSelect();
assert.deepStrictEqual(actionCalls.pop(), ['rnode', 'Radio', true]);

actionRecord.iface.enabled = false;
var resume = actionContext.buildIfaceActionItems('rnode', 'Radio')[0];
assert.strictEqual(resume.label, 'Resume Interface');
resume.onSelect();
assert.deepStrictEqual(actionCalls.pop(), ['rnode', 'Radio', false]);

function labels(items) {
    // Copy into this realm so deepStrictEqual compares values, not prototypes.
    return Array.from(items).filter(function(item) { return !item.separator; })
        .map(function(item) { return item.label; });
}
function bleState(state, reason) {
    actionContext._cachedConfigIfaces.mobile_hardware.ble_rnode = { state: state, reason: reason };
    return actionContext.buildIfaceActionItems('rnode', 'Radio');
}

actionContext._cachedConfigIfaces.mobile_hardware.ble_rnode = { state: 'failed', reason: 'stale_bond' };
assert(!labels(actionContext.buildIfaceActionItems('rnode', 'Radio')).includes('Connection Help'),
    'a paused radio has nothing to troubleshoot');
actionRecord.iface.enabled = true;

var unreachable = bleState('waiting_for_radio', 'not_reachable');
assert.deepStrictEqual(labels(unreachable).slice(0, 2), ['Connecting…', 'Connection Help']);
assert.strictEqual(unreachable[0].disabled, true, 'help never replaces the supervisor-owned retry');
assert.deepStrictEqual(labels(bleState('connecting', 'link_lost')).slice(0, 2),
    ['Connecting…', 'Connection Help']);
var grant = bleState('connecting', 'permission_needed')[0];
assert.strictEqual(grant.label, 'Grant Bluetooth Permission');
var stale = bleState('failed', 'stale_bond');
assert.deepStrictEqual(labels(stale).slice(0, 2), ['Retry Connection', 'Connection Help']);
assert.deepStrictEqual(labels(bleState('failed', 'bond_timeout')).slice(0, 2),
    ['Retry Connection', 'Connection Help']);
var rejected = bleState('failed', 'rejected_tx_power');
assert.deepStrictEqual(labels(rejected).slice(0, 2), ['Retry Connection', 'Connection Help']);
[['connected', ''], ['initializing', ''], ['waiting_for_radio', 'radio_disconnected'],
    ['failed', 'connect_failed']].forEach(function(row) {
    assert(!labels(bleState(row[0], row[1])).includes('Connection Help'), row.join('/'));
});

// A desktop serial RNode has no Android hardware state; its driver explains it.
actionContext._cachedConfigIfaces.mobile_hardware = {};
actionRecord.iface.port = '/dev/cu.usbmodem4101';
function runtimeState(state, reason) {
    actionContext.liveStatus = { online: false, rnode_state: { state: state, reason: reason } };
    return actionContext.buildIfaceActionItems('rnode', 'Radio');
}
var serialRejected = runtimeState('failed', 'rejected_tx_power');
assert.deepStrictEqual(labels(serialRejected).slice(0, 2), ['Retry Connection', 'Connection Help']);
var serialAway = runtimeState('waiting_for_radio', 'not_reachable');
assert.deepStrictEqual(labels(serialAway).slice(0, 2), ['Pause Interface', 'Connection Help'],
    'a retrying driver is paused, never replaced by Resume');
assert.deepStrictEqual(labels(runtimeState('initializing', null)).slice(0, 1), ['Pause Interface']);
var serialDenied = runtimeState('waiting_for_radio', 'port_access_denied');
var serialBusy = runtimeState('waiting_for_radio', 'port_in_use');
// Desktop Bluetooth radios use the same driver state.
actionRecord.iface.port = 'ble://00:11:22:33:44:55';
assert.deepStrictEqual(labels(runtimeState('failed', 'rejected_tx_power')).slice(0, 2),
    ['Retry Connection', 'Connection Help']);
actionContext.liveStatus = null;

function flush() { return new Promise(function(resolve) { setImmediate(resolve); }); }

(async function() {
    unreachable[1].onSelect();
    await flush();
    assert.deepStrictEqual(dialogs.pop(), ['alert', 'Radio not found']);

    stale[1].onSelect();
    await flush();
    assert.deepStrictEqual(dialogs.pop(), ['confirm', 'Pair again']);
    assert.strictEqual(androidCalls.pop(), 'bluetooth');

    grant.onSelect();
    await flush();
    assert.deepStrictEqual(actionCalls.pop(), ['ble-permission']);
    assert.strictEqual(dialogs.length, 0, 'a granted permission needs no settings detour');

    // A rejected setting is fixed by editing the interface, not by retrying.
    rejected[1].onSelect();
    await flush();
    assert.deepStrictEqual(dialogs.pop(), ['confirm', 'TX power not supported']);
    assert.deepStrictEqual(actionCalls.pop(), ['edit', 'rnode', 'Radio']);
    serialRejected[1].onSelect();
    await flush();
    assert.deepStrictEqual(dialogs.pop(), ['confirm', 'TX power not supported']);
    assert.deepStrictEqual(actionCalls.pop(), ['edit', 'rnode', 'Radio']);
    // Wired radios get wired guidance, without Bluetooth steps.
    serialAway[1].onSelect();
    await flush();
    assert.deepStrictEqual(dialogs.pop(), ['alert', 'Radio not found']);
    // A port the OS refuses, or another program holds, says so.
    serialDenied[1].onSelect();
    await flush();
    assert.deepStrictEqual(dialogs.pop(), ['alert', 'Port access denied']);
    serialBusy[1].onSelect();
    await flush();
    assert.deepStrictEqual(dialogs.pop(), ['alert', 'Port in use']);

    // Android stops prompting after repeated denials; Settings is the only path.
    actionContext.permissionGranted = false;
    grant.onSelect();
    await flush();
    assert.deepStrictEqual(actionCalls.pop(), ['ble-permission']);
    assert.deepStrictEqual(dialogs.pop(), ['confirm', 'Bluetooth permission needed']);
    assert.strictEqual(androidCalls.pop(), 'app');

    console.log('Mobile hardware health tests passed');
})().catch(function(error) {
    console.error(error);
    process.exit(1);
});
