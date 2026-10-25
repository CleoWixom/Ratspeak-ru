#!/usr/bin/env node
// A key, phrase or access file is released with its identity's PIN, or after
// a confirmation when it has none. A wrong PIN stays in the PIN sheet.
'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const js = path.join(__dirname, '..', '..', 'dashboard', 'static', 'js');
const identitySource = fs.readFileSync(path.join(js, 'identity.js'), 'utf8');
const stateSource = fs.readFileSync(path.join(js, 'state.js'), 'utf8');

function functionSource(source, name) {
    const start = source.indexOf('function ' + name + '(');
    assert.notEqual(start, -1, name + ' must exist');
    let depth = 0;
    for (let index = source.indexOf('{', start); index < source.length; index++) {
        if (source[index] === '{') depth++;
        if (source[index] === '}' && --depth === 0) return source.slice(start, index + 1);
    }
    throw new Error('unterminated ' + name);
}

const HASH = 'ab'.repeat(16);

function load({ confirm = true } = {}) {
    const seen = { confirms: [], modals: [], closed: 0 };
    const fields = new Map();
    const field = id => {
        if (!fields.has(id)) fields.set(id, { value: '', textContent: '', style: {}, dataset: {} });
        return fields.get(id);
    };
    const context = {
        document: { getElementById: field },
        rsConfirm(opts) {
            seen.confirms.push(opts);
            return Promise.resolve(confirm);
        },
        showIdentityModal(title, body, onConfirm, confirmClass) {
            seen.modals.push({ title, body, onConfirm, confirmClass });
        },
        closeIdentityModal() { seen.closed++; },
        window: {}
    };
    vm.runInNewContext([
        functionSource(stateSource, 'escapeHtml'),
        functionSource(identitySource, 'identityPinError'),
        functionSource(identitySource, 'authorizeIdentityKey')
    ].join('\n'), context, { filename: 'identity.js' });
    return { context, seen, field };
}

function unauthorized(message) {
    const error = new Error(message);
    error.code = 'unauthorized';
    return error;
}

const settle = () => new Promise(resolve => setImmediate(resolve));
const opts = { title: 'Copy Secret', message: 'A <secret>.', confirmText: 'Copy', danger: true };

(async () => {
    // Without a PIN: a confirmation, then `confirmed`.
    let h = load();
    const sent = [];
    let result = await h.context.authorizeIdentityKey({ hash: HASH, passcode_protected: false }, opts,
        auth => { sent.push(auth); return Promise.resolve('released'); });
    assert.equal(result, 'released');
    assert.deepEqual(JSON.parse(JSON.stringify(sent)), [{ confirmed: true }]);
    assert.equal(h.seen.confirms[0].danger, true);
    assert.equal(h.seen.confirms[0].confirmText, 'Copy');

    h = load({ confirm: false });
    result = await h.context.authorizeIdentityKey({ hash: HASH }, opts,
        () => { throw new Error('declining sends nothing'); });
    assert.equal(result, null);

    // With a PIN: the sheet, which keeps a missing or wrong PIN inline.
    h = load();
    const pins = [];
    const replies = [unauthorized('Incorrect PIN'), 'released'];
    const pending = h.context.authorizeIdentityKey({ hash: HASH, passcode_protected: true }, opts, auth => {
        pins.push(auth.passcode);
        const reply = replies.shift();
        return reply instanceof Error ? Promise.reject(reply) : Promise.resolve(reply);
    });
    assert.equal(h.seen.confirms.length, 0);
    const sheet = h.seen.modals[0];
    assert.equal(sheet.title, 'Copy Secret');
    assert.match(sheet.body, /A &lt;secret&gt;\./);
    assert.equal(sheet.confirmClass, 'danger');
    assert.equal(h.field('identity-modal-confirm').textContent, 'Copy');
    assert.equal(sheet.onConfirm(), undefined, 'no PIN, no request');
    assert.equal(h.field('identity-key-error').textContent, 'Enter your PIN.');
    h.field('identity-key-pin').value = 'wrong pin';
    await sheet.onConfirm();
    assert.equal(h.field('identity-key-error').textContent, 'Incorrect PIN');
    assert.equal(h.seen.closed, 0, 'a wrong PIN keeps the sheet open');
    h.field('identity-key-pin').value = 'right pin';
    await sheet.onConfirm();
    await settle();
    assert.deepEqual(pins, ['wrong pin', 'right pin']);
    assert.equal(h.seen.closed, 1);
    assert.equal(await pending, 'released');

    console.log('Identity key authorization tests passed');
})().catch(error => {
    console.error(error);
    process.exit(1);
});
