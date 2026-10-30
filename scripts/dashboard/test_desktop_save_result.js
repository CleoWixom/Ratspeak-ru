#!/usr/bin/env node
// A desktop save reports how it ended: the page waits for the save dialog
// and the WebView, then says the file was saved, failed, or nothing at all
// when the dialog was cancelled, instead of saying the dialog opened.
'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const js = path.join(__dirname, '..', '..', 'dashboard', 'static', 'js');
const identitySource = fs.readFileSync(path.join(js, 'identity.js'), 'utf8');

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

function load({ desktop = true } = {}) {
    const seen = { order: [], toasts: [], handlers: [], unlistened: 0, revoked: [] };
    let next = 0;
    const context = {
        console,
        // Unreferenced, so a pending revoke never keeps the test running.
        setTimeout(callback, ms) {
            const timer = setTimeout(callback, ms);
            timer.unref();
            return timer;
        },
        clearTimeout,
        Promise,
        Blob: function Blob(parts, options) { this.parts = parts; this.type = options && options.type; },
        URL: {
            createObjectURL() { next += 1; return 'blob:tauri://localhost/' + next; },
            revokeObjectURL(url) { seen.revoked.push(url); }
        },
        document: {
            createElement() {
                return {
                    style: {},
                    click() { seen.order.push('click:' + this.href); },
                    remove() {}
                };
            },
            body: { appendChild() {} }
        },
        hasAndroidIdentityDocumentBridge: () => false,
        hasAndroidIdentityExportBridge: () => false,
        bytesToBase64: () => '',
        isTauriDesktop: () => desktop,
        showToast(...args) { seen.toasts.push(args); },
        window: {},
        RS: {
            listen(name, handler) {
                seen.order.push('listen:' + name);
                seen.handlers.push(handler);
                return Promise.resolve(() => { seen.unlistened += 1; });
            }
        }
    };
    context.window.RS = context.RS;
    vm.runInNewContext([
        functionSource(identitySource, 'desktopDownloadResult'),
        functionSource(identitySource, 'saveBytesToUserFile'),
        functionSource(identitySource, 'saveIdentityExportPayload')
    ].join('\n'), context, { filename: 'identity.js' });
    return { context, seen };
}

const settle = () => new Promise(resolve => setImmediate(resolve));
const payload = {
    bytes: new Uint8Array([1]),
    base64: 'AQ==',
    fileName: 'abcd-ratspeak-identity.rsi',
    mimeType: 'application/octet-stream',
    label: 'Ratspeak identity backup'
};

async function saveEndingWith(report) {
    const h = load();
    const saving = h.context.saveIdentityExportPayload(payload);
    await settle();
    // The page listens before the download starts, and says nothing yet.
    assert.deepEqual(h.seen.order, ['listen:download_result', 'click:blob:tauri://localhost/1']);
    assert.deepEqual(h.seen.toasts, []);
    // Another download's report is not this one's.
    h.seen.handlers[0]({ url: 'blob:tauri://localhost/2', status: 'failed' });
    await settle();
    assert.deepEqual(h.seen.toasts, []);
    h.seen.handlers[0](report('blob:tauri://localhost/1'));
    await saving;
    assert.equal(h.seen.unlistened, 1);
    assert.deepEqual(h.seen.revoked, ['blob:tauri://localhost/1']);
    return h.seen.toasts;
}

(async () => {
    let toasts = await saveEndingWith(url => ({ url, status: 'saved' }));
    assert.deepEqual(toasts, [['Ratspeak identity backup saved', 'toast-success', 3000]]);

    toasts = await saveEndingWith(url => ({ url, status: 'failed' }));
    assert.deepEqual(toasts, [['Could not save abcd-ratspeak-identity.rsi', 'toast-error', 5000]]);

    toasts = await saveEndingWith(url => ({ url, status: 'cancelled' }));
    assert.deepEqual(toasts, [], 'a cancelled dialog needs no toast');

    // Off the desktop no report comes, so the save starts and resolves at once.
    const h = load({ desktop: false });
    const result = await h.context.saveBytesToUserFile(payload.bytes, payload.fileName, payload.mimeType);
    assert.deepEqual(h.seen.order, ['click:blob:tauri://localhost/1']);
    assert.equal(result.status, null);

    console.log('desktop save result: ok');
})().catch(error => {
    console.error(error);
    process.exit(1);
});
