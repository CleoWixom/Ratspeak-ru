#!/usr/bin/env node
// Export/import of contacts and Internet/TCP connections (Settings -> Identity).
'use strict';

var assert = require('assert');
var fs = require('fs');
var path = require('path');
var vm = require('vm');

var source = fs.readFileSync(path.join(__dirname, '..', '..', 'dashboard', 'static', 'js', 'contacts_backup.js'), 'utf8');
var window = {};
var context = { window: window, RS: {}, document: undefined, TextEncoder: TextEncoder, Object: Object,
    Number: Number, JSON: JSON, Date: Date, parseInt: parseInt, Promise: Promise, String: String, Array: Array };
window.RS = context.RS;
vm.runInNewContext(source, context, { filename: 'contacts_backup.js' });
var api = context.RS.contactsBackup;
var plain = function(value) { return JSON.parse(JSON.stringify(value)); };

var HASH_A = 'a'.repeat(32), HASH_B = 'b'.repeat(32);
var ifaces = {
    tcp_client: [
        { name: 'Public hub', target_host: 'rns.example.org', target_port: '4242' },
        { name: 'Private', target_host: '10.0.0.5', target_port: 7000, network_name: 'mynet', passphrase: 'secret', ifac_size: '16' },
        { name: 'Bad [name]', target_host: 'x.example', target_port: '1' },
        { name: 'No port', target_host: 'y.example' }
    ],
    backbone_client: [{ name: 'BB', target_host: 'bb.example', target_port: '4965' }],
    tcp_server: [{ name: 'Server', listen_port: '4242' }],
    rnode: [{ name: 'Radio', frequency: '868000000' }]
};
var contacts = [
    { hash: HASH_A.toUpperCase(), display_name: ' Alice ' },
    { hash: HASH_B, display_name: '' },
    { hash: 'zz', display_name: 'broken' }
];

// ---- Export: only clients; secrets are opt-in ----
var without = plain(api.buildBundle(contacts, ifaces, false, new Date('2026-10-25T10:00:00Z')));
assert.strictEqual(without.format, 'ratspeak.contacts-connections.v1');
assert.strictEqual(without.exported_at, '2026-10-25T10:00:00.000Z');
assert.deepStrictEqual(without.contacts, [{ hash: HASH_A, name: 'Alice' }, { hash: HASH_B }]);
assert.deepStrictEqual(without.connections.map(function(c) { return c.name; }),
    ['Public hub', 'Private', 'x.example:1', 'BB']);
var privateConn = without.connections[1];
assert.strictEqual(privateConn.ifac_name, 'mynet');
assert.strictEqual(privateConn.ifac_pass, undefined, 'passphrase omitted by default');
assert.strictEqual(privateConn.ifac_pass_omitted, true);
assert.strictEqual(privateConn.ifac_size, 16);
assert(!JSON.stringify(without).includes('secret'));
assert.strictEqual(without.connections[3].type, 'backbone');
var withSecrets = plain(api.buildBundle(contacts, ifaces, true));
assert.strictEqual(withSecrets.connections[1].ifac_pass, 'secret');

// ---- Import: parse and validate ----
var text = JSON.stringify(withSecrets);
var parsed = plain(api.parseBundle(text));
assert.strictEqual(parsed.contacts.length, 2);
assert.strictEqual(parsed.connections.length, 4);
assert.strictEqual(parsed.rejected, 0);
assert.strictEqual(parsed.connections[1].ifac_pass, 'secret');
assert.strictEqual(parsed.connections[1].ifac_size, 16);

['', '   ', 'not json', '[]', '{"format":"other"}', '{"format":"ratspeak.contacts-connections.v1","contacts":{}}']
    .forEach(function(bad) { assert.throws(function() { api.parseBundle(bad); }); });
assert.throws(function() { api.parseBundle('x'.repeat(2 * 1024 * 1024 + 1)); }, /too large/);
assert.throws(function() {
    api.parseBundle(JSON.stringify({ format: api.FORMAT, contacts: new Array(5001).fill({ hash: HASH_A }) }));
}, /too many/);

var hostile = plain(api.parseBundle(JSON.stringify({ format: api.FORMAT,
    contacts: [{ hash: 'nothex' }, { hash: HASH_A, name: 'ok' }, { hash: HASH_A }, { hash: HASH_B, name: 'a\nb' }],
    connections: [
        { type: 'tcp', host: 'a.example', port: 4242 },
        { type: 'tcp', host: 'a.example\nenabled = no', port: 4242 },
        { type: 'tcp', host: 'a.example', port: 0 },
        { type: 'tcp', host: 'a.example', port: '80' },
        { type: 'tcp', host: 'a b', port: 80 },
        { type: 'udp', host: 'a.example', port: 80 },
        { type: 'tcp', host: 'a.example', port: 80, name: '[[x]]' },
        { type: 'tcp', host: 'a.example', port: 80, ifac_pass: 'a#b' },
        { type: 'tcp', host: 'a.example', port: 80, ifac_name: 'x'.repeat(129) },
        { type: 'tcp', host: 'a.example', port: 80, ifac_size: 65 },
        { type: 'tcp', host: '2001:db8::1', port: 80 },
        null
    ] })));
assert.deepStrictEqual(hostile.contacts.map(function(c) { return c.hash; }), [HASH_A]);
assert.deepStrictEqual(hostile.connections.map(function(c) { return c.host; }), ['a.example', '2001:db8::1']);
assert.strictEqual(hostile.rejected, 3 + 10);

// ---- Plan: skip what exists, keep names unique ----
var bundle = api.parseBundle(JSON.stringify({ format: api.FORMAT,
    contacts: [{ hash: HASH_A, name: 'Alice' }, { hash: HASH_B, name: 'Bob' }],
    connections: [
        { type: 'tcp', host: 'RNS.example.org', port: 4242, name: 'Dup endpoint' },
        { type: 'tcp', host: 'new1.example', port: 4242, name: 'Public hub' },
        { type: 'tcp', host: 'new2.example', port: 4242, name: 'Public hub' },
        { type: 'tcp', host: 'new2.example', port: 4242, name: 'Same twice' },
        { type: 'backbone', host: 'bb2.example', port: 4965, name: 'BB', ifac_name: 'n', ifac_pass: 'p', ifac_size: 8 }
    ] }));
var plan = plain(api.planImport(bundle, [{ hash: HASH_A }], ifaces));
assert.deepStrictEqual(plan.contacts.map(function(c) { return c.name; }), ['Bob']);
assert.strictEqual(plan.skippedContacts, 1);
assert.strictEqual(plan.skippedConnections, 2);
assert.deepStrictEqual(plan.connections.map(function(c) { return c.name; }),
    ['Public hub (2)', 'Public hub (3)', 'BB (2)']);

// ---- Run: sequential, per-item failures do not stop the rest ----
async function main() {
    var calls = [];
    var progress = [];
    var summary = plain(await api.runImport(plan, function(command, payload) {
        calls.push([command, plain(payload)]);
        if (command === 'add_backbone_connection') return Promise.reject(new Error('desktop only'));
        return Promise.resolve({});
    }, function(done, total, label) { progress.push(done + '/' + total + ' ' + label); }));
    assert.deepStrictEqual(calls.map(function(c) { return c[0]; }),
        ['add_contact', 'add_tcp_connection', 'add_tcp_connection', 'add_backbone_connection']);
    assert.deepStrictEqual(calls[0][1], { args: { hash: HASH_B, display_name: 'Bob' } });
    assert.deepStrictEqual(calls[1][1], { args: { host: 'new1.example', port: 4242, name: 'Public hub (2)',
        ifac_enabled: false, ifac_network_name: '', ifac_passphrase: '' } });
    assert.strictEqual(calls[2][1].args.name, 'Public hub (3)');
    assert.deepStrictEqual(calls[3][1].args, { host: 'bb2.example', port: 4965, name: 'BB (2)',
        ifac_enabled: true, ifac_network_name: 'n', ifac_passphrase: 'p', ifac_size: 8 });
    assert.strictEqual(summary.contactsAdded, 1);
    assert.strictEqual(summary.connectionsAdded, 2);
    assert.deepStrictEqual(summary.failed, ['BB (2): desktop only']);
    assert.strictEqual(progress.length, 4);
    assert.strictEqual(progress[3], '4/4 BB (2)');
    console.log('contacts backup passed');
}
main().catch(function(error) { console.error(error); process.exit(1); });
