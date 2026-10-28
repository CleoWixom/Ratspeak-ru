#!/usr/bin/env node
// Ratspeak-ru overlay: the loader, the menu/listener wrappers, and the anchors in
// upstream files they depend on. If a Ratspeak update renames one of these anchors,
// this test names it instead of a fork feature silently disappearing.
'use strict';

var assert = require('assert');
var fs = require('fs');
var path = require('path');
var vm = require('vm');

var root = path.join(__dirname, '..', '..');
var read = function(p) { return fs.readFileSync(path.join(root, p), 'utf8'); };

// ---- Anchors in upstream files ----
var anchors = [
    ['dashboard/static/js/health.js', 'function buildIfaceActionItems(ifaceType, ifaceName)'],
    ['dashboard/static/js/health.js', "label: 'Rename'"],
    ['dashboard/static/js/health.js', 'function isLoraInterfaceType'],
    ['dashboard/static/js/lxmf.js', "label: 'Copy LXMF Hash'"],
    ['dashboard/static/js/lxmf.js', 'var lxmfActiveContact'],
    ['dashboard/static/js/connections.js', "label: 'Block'"],
    ['dashboard/static/js/connections.js', "id=\"conn-sheet-more-btn\""],
    ['dashboard/static/js/connections.js', 'function closeConnectionDetailSheet'],
    ['dashboard/static/js/action_popover.js', 'window.actionPopover = function(trigger, items'],
    ['dashboard/static/js/modals.js', 'function openConnectModal'],
    ['dashboard/static/js/modals.js', 'function setConnectTab'],
    ['dashboard/static/js/modals.js', 'function openRnodeModal'],
    ['dashboard/static/js/modals.js', 'function _rnodeApplyRadioControls'],
    ['dashboard/index.html', 'id="settings-backup-identity-btn"'],
    ['dashboard/index.html', 'id="network-activity"'],
    ['dashboard/index.html', 'name="settings-text-scale" value="100"'],
    ['dashboard/static/js/text_scale.js', 'window.RS.textScale = {'],
    ['dashboard/index.html', '/static/js/ru_loader.js']
];
anchors.forEach(function(pair) {
    assert(read(pair[0]).indexOf(pair[1]) !== -1, 'upstream anchor missing in ' + pair[0] + ': ' + pair[1]);
});

// ---- ru_extensions.js ----
function extensionsContext(extra) {
    var calls = [];
    var context = Object.assign({
        window: {}, RS: {}, Array: Array, Date: Date, console: console, calls: calls,
        isLoraInterfaceType: function(type) { return type === 'rnode'; },
        openConnectionQrShare: function(t, n) { calls.push(['qr', t, n]); },
        openRouteSheet: function(h, n) { calls.push(['route', h, n]); },
        closeConnectionDetailSheet: function() { calls.push(['close']); },
        _connsView: function() { return [{ hash: 'aa'.repeat(16), display_name: 'Alice' }]; },
        lxmfActiveContact: 'bb'.repeat(16),
        lxmfContacts: [{ hash: 'bb'.repeat(16), display_name: 'Bob' }],
        announceRadarUpdate: function(p) { calls.push(['radar-update', p.length]); },
        announceRadarPulse: function(d) { calls.push(['radar-pulse', d.hash]); }
    }, extra || {});
    context.window.RS = context.RS;
    vm.runInNewContext(read('dashboard/static/js/ru_extensions.js'), context, { filename: 'ru_extensions.js' });
    return context;
}
var labels = function(items) { return items.map(function(i) { return i.label; }); };
var ext = extensionsContext().RS.ruExtensions;

// QR item sits before Rename, only for shareable connection types.
var iface = [{ label: 'Edit' }, { label: 'Rename' }, { label: 'Remove' }];
assert.deepStrictEqual(labels(ext.withQrItem(iface, 'tcp_client', 'Hub')), ['Edit', 'Share QR code', 'Rename', 'Remove']);
assert.deepStrictEqual(labels(ext.withQrItem(iface, 'rnode', 'Radio')), ['Edit', 'Share QR code', 'Rename', 'Remove']);
assert.deepStrictEqual(labels(ext.withQrItem(iface, 'tcp_server', 'Srv')), ['Edit', 'Rename', 'Remove']);
assert.strictEqual(iface.length, 3, 'input is not mutated');
assert.deepStrictEqual(labels(ext.withQrItem([{ label: 'Edit' }], 'backbone_client', 'B')), ['Edit', 'Share QR code']);
assert.deepStrictEqual(labels(ext.withQrItem(ext.withQrItem(iface, 'tcp_client', 'Hub'), 'tcp_client', 'Hub')).filter(function(l) { return l === 'Share QR code'; }).length, 1);

// Route item in the conversation menu and the contact sheet menu; nothing elsewhere.
var chat = ext.withRouteItem([{ label: 'About' }, { label: 'Copy LXMF Hash' }], null);
assert.deepStrictEqual(labels(chat), ['About', 'Route', 'Copy LXMF Hash']);
var more = ext.withRouteItem([{ label: 'Block' }], { id: 'conn-sheet-more-btn', dataset: { hash: 'aa'.repeat(16) } });
assert.deepStrictEqual(labels(more), ['Route', 'Block']);
assert.deepStrictEqual(labels(ext.withRouteItem([{ label: 'Block' }], { id: 'other' })), ['Block']);
assert.deepStrictEqual(labels(ext.withRouteItem([{ label: 'Delete' }], null)), ['Delete']);
assert.strictEqual(ext.withRouteItem('nope', null), 'nope');

// Selecting the entries calls the fork screens with the right arguments.
var ctx = extensionsContext();
var routeFromChat = ctx.RS.ruExtensions.withRouteItem([{ label: 'Copy LXMF Hash' }], null)[0];
routeFromChat.onSelect();
assert.deepStrictEqual(JSON.parse(JSON.stringify(ctx.calls.pop())), ['route', 'bb'.repeat(16), 'Bob']);
var routeFromSheet = ctx.RS.ruExtensions.withRouteItem([{ label: 'Block' }], { id: 'conn-sheet-more-btn', dataset: { hash: 'aa'.repeat(16) } })[0];
routeFromSheet.onSelect();
assert.deepStrictEqual(JSON.parse(JSON.stringify(ctx.calls.splice(0))), [['close'], ['route', 'aa'.repeat(16), 'Alice']]);

// Wrapping upstream globals and subscribing to events.
var seen = [];
var wrapCtx = {
    window: {}, RS: { listen: function(name, handler) { seen.push([name, handler]); return Promise.resolve(); } },
    Array: Array, Date: Date, console: console, isLoraInterfaceType: function() { return false; },
    openConnectionQrShare: function() {}, openRouteSheet: function() {},
    announceRadarUpdate: function(p) { wrapCtx.updated = p.length; }, announceRadarPulse: function(d) { wrapCtx.pulsed = d.hash; }
};
wrapCtx.window.RS = wrapCtx.RS;
var popoverArgs = null;
wrapCtx.window.buildIfaceActionItems = function() { return [{ label: 'Rename' }]; };
wrapCtx.window.actionPopover = function(trigger, items) { popoverArgs = [trigger, items]; return 'shown'; };
vm.runInNewContext(read('dashboard/static/js/ru_extensions.js'), wrapCtx);
assert.deepStrictEqual(labels(wrapCtx.window.buildIfaceActionItems('tcp_client', 'Hub')), ['Share QR code', 'Rename']);
assert.strictEqual(wrapCtx.window.actionPopover({ id: 'x' }, [{ label: 'A' }]), 'shown');
assert.deepStrictEqual(labels(popoverArgs[1]), ['A']);
assert.deepStrictEqual(seen.map(function(s) { return s[0]; }), ['stats_update', 'announce_received']);
seen[0][1]({ path_table: [1, 2, 3] });
seen[1][1]({ hash: 'abc' });
assert.strictEqual(wrapCtx.updated, 3);
assert.strictEqual(wrapCtx.pulsed, 'abc');
// Installing twice must not wrap twice.
var before = wrapCtx.window.buildIfaceActionItems;
wrapCtx.RS.ruExtensions.install();
assert.strictEqual(wrapCtx.window.buildIfaceActionItems, before);

// ---- ru_loader.js ----
function fakeDom(anchors) {
    var appended = [], styles = [], html = {}, registry = {};
    function element(tag) { return { tag: tag, style: {}, setAttribute: function() {} }; }
    var row = { insertAdjacentHTML: function(where, text) { html.row = [where, text]; } };
    var activity = { insertAdjacentHTML: function(where, text) { html.activity = [where, text]; } };
    var presetLabel = { insertAdjacentHTML: function(where, text) { html.scale = [where, text]; } };
    var document = {
        currentScript: { src: '/static/js/ru_loader.js?v=ui-test-1' },
        head: { appendChild: function(node) { styles.push(node); } },
        body: { appendChild: function(node) {
            appended.push(node);
            if (node.tag === 'script') setTimeout(function() { node.onload(); }, 0);
        } },
        createElement: element,
        querySelector: function(sel) {
            if (sel.indexOf('value="100"') !== -1 && anchors.scale) return { closest: function() { return presetLabel; } };
            return null;
        },
        getElementById: function(id) {
            if (registry[id]) return registry[id];
            if (id === 'settings-backup-identity-btn' && anchors.settings) return { closest: function() { return row; } };
            if (id === 'network-activity' && anchors.activity) return activity;
            return null;
        }
    };
    return { document: document, appended: appended, styles: styles, html: html };
}
async function runLoader(anchors) {
    var dom = fakeDom(anchors), warnings = [];
    vm.runInNewContext(read('dashboard/static/js/ru_loader.js'), { document: dom.document,
        window: { RS: { diag: function(level, tag) { assert.strictEqual(level, 'warn'); warnings.push(tag); } } },
        setTimeout: setTimeout });
    for (var i = 0; i < 30; i++) await new Promise(function(r) { setTimeout(r, 1); });
    dom.warnings = warnings;
    return dom;
}
(async function() {
    var full = await runLoader({ settings: true, activity: true, scale: true });
    assert.strictEqual(full.styles[0].href, '/static/css/ratspeak-ru.css?v=ui-test-1');
    assert.strictEqual(full.html.row[0], 'afterend');
    assert(full.html.row[1].indexOf('settings-export-contacts-btn') !== -1 && full.html.row[1].indexOf('settings-import-contacts-btn') !== -1);
    assert.strictEqual(full.html.activity[0], 'afterbegin');
    assert(full.html.activity[1].indexOf('announce-radar-canvas') !== -1);
    var scripts = full.appended.filter(function(n) { return n.tag === 'script'; }).map(function(n) { return n.src; });
    assert.deepStrictEqual(scripts, [
        '/static/js/connection_link_presenters.js?v=ui-test-1', '/static/js/native_connection_link.js?v=ui-test-1',
        '/static/js/connection_qr.js?v=ui-test-1', '/static/js/contacts_backup.js?v=ui-test-1',
        '/static/js/announce_radar.js?v=ui-test-1', '/static/js/route_lookup.js?v=ui-test-1',
        '/static/js/ru_text_scale.js?v=ui-test-1', '/static/js/ru_extensions.js?v=ui-test-1']);
    assert.strictEqual(full.html.scale[0], 'beforebegin');
    ['50', '70', '90'].forEach(function(v) { assert(full.html.scale[1].indexOf('value="' + v + '"') !== -1); });
    assert(full.appended.some(function(n) { return n.tag === 'input' && n.id === 'contacts-backup-file-input'; }));
    assert.strictEqual(full.warnings.length, 0);

    // Missing anchors cost one feature each, never the loader.
    var bare = await runLoader({});
    assert.strictEqual(bare.warnings.length, 3);
    assert.strictEqual(bare.appended.filter(function(n) { return n.tag === 'script'; }).length, 8);
    console.log('fork overlay passed');
})().catch(function(error) { console.error(error); process.exit(1); });

// ---- ru_text_scale.js: 50/70/90 presets on top of upstream text_scale.js ----
(function() {
    var store = {};
    var root = { style: {}, dataset: {} };
    var ctx = {
        window: { CustomEvent: function(n, o) { this.type = n; this.detail = o.detail; }, dispatchEvent: function() {}, RS: {} },
        document: { documentElement: root, addEventListener: function() {} },
        localStorage: { getItem: function(k) { return k in store ? store[k] : null; },
                        setItem: function(k, v) { store[k] = String(v); },
                        removeItem: function(k) { delete store[k]; } },
        console: console
    };
    ctx.CustomEvent = ctx.window.CustomEvent;
    ctx.window.document = ctx.document; ctx.window.localStorage = ctx.localStorage;
    vm.createContext(ctx);
    vm.runInContext(read('dashboard/static/js/text_scale.js'), ctx);
    ctx.RS = ctx.window.RS;
    vm.runInContext(read('dashboard/static/js/ru_text_scale.js').replace('window.RS', 'window.RS'), ctx);
    var ts = ctx.window.RS.textScale;
    assert.strictEqual(ts.normalize(60), 50);
    assert.strictEqual(ts.normalize(80), 70);
    assert.strictEqual(ts.normalize(104), 100);
    assert.strictEqual(ts.commit(70), 70);
    assert.strictEqual(root.style.fontSize, '70%');
    assert.strictEqual(ts.commit(100), 70, 'backend echo of 100 keeps the small choice');
    assert.strictEqual(ts.get(), 70);
    delete store['ratspeak-ru-text-scale'];
    assert.strictEqual(ts.commit(120), 120);
    assert.strictEqual(ts.get(), 120);
    console.log('ru_text_scale ok');
})();
