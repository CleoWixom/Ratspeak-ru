#!/usr/bin/env node
// Previews: the row shows the stored setting, sends the one chosen, and goes
// back to the old one when the change is refused.
'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const source = fs.readFileSync(
    path.join(__dirname, '..', '..', 'dashboard', 'static', 'js', 'settings.js'),
    'utf8'
);

// The row sits with the notification toggle and follows it.
const html = fs.readFileSync(path.join(__dirname, '..', '..', 'dashboard', 'index.html'), 'utf8');
assert.match(html, /<div class="settings-row" id="settings-row-notification-preview" style="display:none;">/);
assert.match(html, /<button class="selector-badge" id="notification-preview-select" type="button">Name and message<\/button>/);
assert.ok(html.indexOf('id="settings-row-notifications"') < html.indexOf('id="settings-row-notification-preview"'));
assert.match(source, /_preview\.show\(data\.preview\);/);
assert.match(source, /_preview\.visible\(_notifPreferenceEnabled\);/);
assert.match(source, /_preview\.visible\(enabled\);/);

function functionSource(name) {
    const start = source.indexOf('function ' + name + '(');
    assert.notEqual(start, -1, name + ' must exist');
    let depth = 0;
    for (let index = source.indexOf('{', start); index < source.length; index++) {
        if (source[index] === '{') depth++;
        if (source[index] === '}' && --depth === 0) return source.slice(start, index + 1);
    }
    throw new Error('unterminated ' + name);
}

function labels() {
    const start = source.indexOf('var NOTIFICATION_PREVIEW_LABELS = {');
    assert.notEqual(start, -1);
    return source.slice(start, source.indexOf('};', start) + 2);
}

const settle = () => new Promise(resolve => setImmediate(resolve));

function load({ choice, reply }) {
    const seen = { invoked: [], toasts: [], choices: [] };
    const badge = {
        textContent: '', attributes: {}, click: null,
        setAttribute(name, value) { this.attributes[name] = value; },
        getAttribute(name) { return this.attributes[name]; },
        addEventListener(name, fn) { if (name === 'click') this.click = fn; }
    };
    const row = { style: { display: 'none' } };
    const context = {
        rsChoice(opts) { seen.choices.push(opts); return Promise.resolve(choice); },
        RS: {
            invoke(name, args) {
                seen.invoked.push([name, JSON.parse(JSON.stringify(args))]);
                return reply;
            }
        },
        showToast(...args) { seen.toasts.push(args); },
        toastErrorCopy: (error, fallback) => fallback
    };
    vm.runInNewContext(labels() + '\n' + functionSource('_initNotificationPreview') +
        '\nvar controls = _initNotificationPreview(row, badge);', Object.assign(context, { row, badge }));
    return { controls: context.controls, badge, row, seen };
}

(async () => {
    let h = load({ choice: 'name_only', reply: Promise.resolve({ preview: 'name_only' }) });
    h.controls.show(undefined);
    assert.equal(h.badge.textContent, 'Name and message', 'no setting means the default');
    h.controls.show('neither');
    assert.equal(h.badge.textContent, 'Neither');
    h.controls.visible(true);
    assert.equal(h.row.style.display, '');
    h.controls.visible(false);
    assert.equal(h.row.style.display, 'none', 'no previews to choose with alerts off');

    h.controls.show('name_and_message');
    h.badge.click();
    await settle();
    assert.deepEqual(h.seen.invoked, [['set_notification_preview', { preview: 'name_only' }]]);
    assert.equal(h.badge.textContent, 'Name only');
    assert.match(h.seen.choices[0].message, /call always shows who is calling/);

    h = load({ choice: 'neither', reply: Promise.reject({ code: 'internal_error', message: 'disk full' }) });
    h.controls.show('name_and_message');
    h.badge.click();
    await settle();
    await settle();
    assert.equal(h.badge.textContent, 'Name and message', 'a refused change goes back');
    assert.deepEqual(h.seen.toasts, [['Could not change previews', 'toast-error', 4000]]);

    h = load({ choice: null, reply: Promise.resolve({}) });
    h.controls.show('name_only');
    h.badge.click();
    await settle();
    assert.deepEqual(h.seen.invoked, [], 'closing the choice changes nothing');
    assert.equal(h.badge.textContent, 'Name only');

    console.log('Notification preview tests passed');
})().catch(error => {
    console.error(error);
    process.exit(1);
});
