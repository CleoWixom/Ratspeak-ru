#!/usr/bin/env node
// Links built for "Share QR code" from saved Internet/TCP and Radio interfaces.
'use strict';

var assert = require('assert');
var fs = require('fs');
var path = require('path');
var vm = require('vm');

var source = fs.readFileSync(path.join(__dirname, '..', '..', 'dashboard', 'static', 'js', 'connection_qr.js'), 'utf8');
var window = {};
var context = { window: window, RS: {}, TextEncoder: TextEncoder, unescape: unescape,
    encodeURIComponent: encodeURIComponent };
window.RS = context.RS;
vm.runInNewContext(source, context, { filename: 'connection_qr.js' });
var qr = context.RS.connectionQr;
assert(qr && typeof window.openConnectionQrShare === 'function');

function links(type, iface) {
    var plan = qr.candidatesFromInterface(type, iface);
    assert(!plan.error, plan.error);
    return JSON.parse(JSON.stringify(plan)); // plain objects from this realm
}

// TCP with IFAC and a name.
var tcp = links('tcp_client', { name: 'My node', target_host: 'rns.example.org', target_port: '4242',
    network_name: 'mynet', passphrase: 'secret phrase' });
assert.strictEqual(tcp.kind, 'tcp');
assert.strictEqual(tcp.hasSecret, true);
assert.strictEqual(tcp.summary, 'rns.example.org:4242');
assert.deepStrictEqual(tcp.links, [
    'ratspeak://tcp?v=1&host=rns.example.org&port=4242&name=My%20node&ifac_name=mynet&ifac_pass=secret%20phrase',
    'ratspeak://tcp?v=1&host=rns.example.org&port=4242&ifac_name=mynet&ifac_pass=secret%20phrase']);

// Alternate IFAC key spellings, backbone client, no IFAC, numeric port.
var backbone = links('backbone_client', { name: 'B', target_host: '10.0.0.5', target_port: 7000,
    networkname: 'n', pass_phrase: 'p' });
assert(backbone.links[0].includes('ifac_name=n&ifac_pass=p'));
var plain = links('tcp_client', { target_host: 'a.example', target_port: '1' });
assert.strictEqual(plain.hasIfac, false);
assert.deepStrictEqual(plain.links, ['ratspeak://tcp?v=1&host=a.example&port=1']);

// Missing data is reported, not guessed.
assert(qr.candidatesFromInterface('tcp_client', { target_host: 'a.example' }).error);
assert(qr.candidatesFromInterface('tcp_server', { listen_port: '4242' }).error);
assert(qr.candidatesFromInterface('rnode', { frequency: '868000000' }).error);

// Radio: exact custom parameters, mode and airtime limits.
var radio = links('rnode', { name: 'Local mesh', frequency: '868000000', bandwidth: '250000',
    spreadingfactor: '9', codingrate: '5', txpower: '14', mode: 'Gateway',
    airtime_limit_short: '1.5', airtime_limit_long: '10', port: 'ble://AA:BB' });
assert.strictEqual(radio.kind, 'radio');
assert.deepStrictEqual(radio.links[1],
    'ratspeak://radio?v=1&freq=868000000&bw=250000&sf=9&cr=5&tx=14&mode=gateway&airtime_short=1.5&airtime_long=10');
assert(radio.links[0].startsWith('ratspeak://radio?v=1&name=Local%20mesh&freq='));
assert(!radio.links.join('').includes('ble'), 'device port is never shared');
assert.strictEqual(radio.summary, '868 MHz · 250 kHz · SF9 · CR5');
var noMode = links('rnode', { frequency: 433000000, bandwidth: 125000, spreadingfactor: 7,
    codingrate: 5, txpower: 10, mode: 'weird', airtime_limit_long: '0' });
assert.deepStrictEqual(noMode.links, ['ratspeak://radio?v=1&freq=433000000&bw=125000&sf=7&cr=5&tx=10']);

// Resolution: Rust rejection of the name falls back to the bare link.
async function main() {
    var asked = [];
    var accepted = await qr.resolve('tcp_client', { name: 'Bad [name]', target_host: 'a.example',
        target_port: '4242' }, function(link) {
        asked.push(link);
        return /name=/.test(link) ? Promise.reject(new Error('name is not valid')) : Promise.resolve();
    });
    assert.strictEqual(accepted.link, 'ratspeak://tcp?v=1&host=a.example&port=4242');
    assert.strictEqual(asked.length, 2);

    // Everything rejected: the reason reaches the caller.
    await assert.rejects(qr.resolve('tcp_client', { target_host: 'a.example', target_port: '1' },
        function() { return Promise.reject(new Error('Connection link needs a valid host')); }),
        /valid host/);

    // Too long for the QR layout: the name is dropped; still too long: error.
    var longName = 'N'.repeat(60);
    var fit = await qr.resolve('tcp_client', { name: longName, target_host: 'a.example', target_port: '1',
        network_name: 'x'.repeat(60), passphrase: 'y'.repeat(60) }, function() { return Promise.resolve(); });
    assert(!fit.link.includes('name=' + longName));
    assert(new TextEncoder().encode(fit.link).length <= qr.maxBytes);
    await assert.rejects(qr.resolve('tcp_client', { target_host: 'a.example', target_port: '1',
        network_name: 'x'.repeat(128), passphrase: 'y'.repeat(128) },
        function() { return Promise.resolve(); }), /too long for a QR code/);

    // Non-ASCII names are percent-encoded to UTF-8 bytes.
    var cyr = links('tcp_client', { name: 'Узел', target_host: 'a.example', target_port: '4242' });
    assert(cyr.links[0].includes('name=%D0%A3%D0%B7%D0%B5%D0%BB'));

    if (process.env.CONNECTION_QR_DUMP) {
        var all = [tcp, backbone, plain, radio, noMode, cyr].reduce(function(list, plan) {
            return list.concat(plan.links);
        }, []);
        fs.writeFileSync(process.env.CONNECTION_QR_DUMP, all.join('\n') + '\n');
    }
    console.log('connection QR links passed');
}
main().catch(function(error) { console.error(error); process.exit(1); });
