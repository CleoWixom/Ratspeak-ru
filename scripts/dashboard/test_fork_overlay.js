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
        '/static/js/ru_text_scale.js?v=ui-test-1', '/static/js/ru_peer_views.js?v=ui-test-1', '/static/js/ru_lxma.js?v=ui-test-1', '/static/js/ru_geo.js?v=ui-test-1', '/static/js/ru_extensions.js?v=ui-test-1']);
    assert.strictEqual(full.html.scale[0], 'beforebegin');
    ['50', '70', '90'].forEach(function(v) { assert(full.html.scale[1].indexOf('value="' + v + '"') !== -1); });
    assert(full.appended.some(function(n) { return n.tag === 'input' && n.id === 'contacts-backup-file-input'; }));
    assert.strictEqual(full.warnings.length, 0);

    // Missing anchors cost one feature each, never the loader.
    var bare = await runLoader({});
    assert.strictEqual(bare.warnings.length, 3);
    assert.strictEqual(bare.appended.filter(function(n) { return n.tag === 'script'; }).length, 11);
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

// ---- ru_peer_views.js: pure grouping plan ----
(function() {
    var ctx = { window: { renderDashboardPeersList: undefined }, document: { readyState: 'complete' }, localStorage: { getItem: function() { return null; } }, navigator: { language: 'en' } };
    vm.createContext(ctx);
    vm.runInContext(read('dashboard/static/js/ru_peer_views.js'), ctx);
    var v = ctx.window.RuPeerViews;
    assert.strictEqual(JSON.stringify(v.MODES), JSON.stringify(['default', 'status', 'contacts', 'list', 'tiles']));
    var items = [
        { hash: 'a', status: 'direct', is_contact: true }, { hash: 'b', status: 'stale', is_contact: false },
        { hash: 'c', status: 'unknown', is_contact: true }, { hash: 'd', status: 'reachable', is_contact: false }
    ];
    var byStatus = v.planGroups(items, 'status');
    assert.strictEqual(byStatus.map(function(g) { return g.key + ':' + g.hashes.join(''); }).join('|'), 'reachable:ad|stale:b|offline:c');
    var byContacts = v.planGroups(items, 'contacts');
    assert.strictEqual(byContacts.map(function(g) { return g.key + ':' + g.hashes.join(''); }).join('|'), 'contact:ac|other:bd');
    assert.strictEqual(v.planGroups([], 'status').length, 0);
    console.log('ru_peer_views ok');
})();

// ---- ru_lxma.js: parser and native-target presenter ----
(function() {
    var prompts = [], invoked = [];
    var doc = { addEventListener: function() {} };
    var ctx = {
        window: { RS: { invokeOrToast: function(cmd, args) { invoked.push([cmd, args]); } } },
        document: doc, Event: function() {},
        rsPromptContact: function(opts) { prompts.push(opts); return Promise.resolve({ hash: opts.defaultHash, display_name: 'Bob' }); }
    };
    ctx.window.openNativeConnectionLink = function() { return 'upstream'; };
    vm.createContext(ctx);
    vm.runInContext(read('dashboard/static/js/ru_lxma.js'), ctx);
    var parse = ctx.window.RuLxma.parse;
    var h = '0123456789abcdef0123456789abcdef', k = 'ab'.repeat(64);
    var a = parse('lxma://' + h);
    assert.strictEqual(a.hash, h); assert.strictEqual(a.publicKey, null);
    var b = parse('LXMA://' + h.toUpperCase() + '.' + k);
    assert.strictEqual(b.hash, h); assert.strictEqual(b.publicKey, k);
    assert(parse('lxma://' + h + '/'));
    ['lxma://', 'lxma://abc', 'lxma://' + h + '00', 'lxma://' + h + '.', 'lxma://' + h + '.' + k.slice(1),
     'lxma://' + h + '.' + k + '.' + k, 'lxma://' + h + '?x=1', ' lxma://' + h, 'https://example.com', 'lxma://' + h.slice(1) + 'z'
    ].forEach(function(bad) { assert.strictEqual(parse(bad), null, bad); });
    assert.strictEqual(ctx.window.openNativeConnectionLink({ kind: 'tcp' }), 'upstream');
    assert.strictEqual(ctx.window.openNativeConnectionLink({ kind: 'lxma', hash: h, public_key: null }), true);
    assert.strictEqual(prompts[0].defaultHash, h);
    Promise.resolve().then(function() {}).then(function() {
        assert.strictEqual(invoked[0][0], 'add_contact');
        assert.strictEqual(invoked[0][1].args.hash, h);
        console.log('ru_lxma ok');
    });
})();

// ---- ru_geo.js: fix validation, throttling, config ----
(function() {
    var store = {};
    var ctx = {
        window: { addEventListener: function() {}, dispatchEvent: function() {}, CustomEvent: function() {} },
        document: { readyState: 'complete', getElementById: function() { return null; } },
        localStorage: { getItem: function(k) { return k in store ? store[k] : null; }, setItem: function(k, v) { store[k] = String(v); } },
        navigator: { language: 'en' }, setInterval: function() {}, Date: Date, Math: Math, JSON: JSON,
        CustomEvent: function() {}
    };
    vm.createContext(ctx);
    vm.runInContext(read('dashboard/static/js/ru_geo.js'), ctx);
    var geo = ctx.window.RuGeo, p = geo._pure;
    assert.strictEqual(p.normalizeFix({ latitude: 91, longitude: 0 }, 'phone', 1), null);
    assert.strictEqual(p.normalizeFix({ latitude: 0, longitude: 181 }, 'phone', 1), null);
    assert.strictEqual(p.normalizeFix({ latitude: 'x', longitude: 0 }, 'phone', 1), null);
    assert.strictEqual(p.normalizeFix(null, 'phone', 1), null);
    var fix = p.normalizeFix({ lat: 55.75, lon: 37.62, acc: 12 }, 'radio', 5);
    assert.strictEqual(fix.lat, 55.75); assert.strictEqual(fix.acc, 12); assert.strictEqual(fix.source, 'radio'); assert.strictEqual(fix.alt, null);
    var d = p.distanceM({ lat: 0, lon: 0 }, { lat: 0, lon: 1 });
    assert(d > 111000 && d < 111400, String(d));
    var a = { lat: 10, lon: 10, ts: 0 };
    assert.strictEqual(p.shouldAccept(null, a), true);
    assert.strictEqual(p.shouldAccept(a, { lat: 10, lon: 10.00001, ts: 1000 }), false);
    assert.strictEqual(p.shouldAccept(a, { lat: 10.001, lon: 10, ts: 1000 }), true);
    assert.strictEqual(p.shouldAccept(a, { lat: 10, lon: 10, ts: 5000 }), true);
    assert.strictEqual(geo.config().enabled, false);
    geo.setSource('radio');
    assert.strictEqual(geo.config().source, 'radio');
    assert.strictEqual(geo.pushRadioFix({ lat: 1, lon: 1 }), false, 'ignored while disabled');
    geo.setEnabled(true);
    assert.strictEqual(geo.pushRadioFix({ lat: 1, lon: 1 }), true);
    assert.strictEqual(geo.get().source, 'radio');
    geo.setEnabled(false);
    assert.strictEqual(geo.get(), null);
    assert.strictEqual(JSON.parse(store['ratspeak-ru-geo']).enabled, false);
    console.log('ru_geo ok');
})();
