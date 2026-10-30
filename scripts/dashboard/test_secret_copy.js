#!/usr/bin/env node
// Keys and phrases are copied by the native side, which keeps them out of
// clipboard history and clears them after a minute; a failed copy says so
// and never falls back to a copy that stays.
'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const js = path.join(__dirname, '..', '..', 'dashboard', 'static', 'js');
const stateSource = fs.readFileSync(path.join(js, 'state.js'), 'utf8');
const toastSource = fs.readFileSync(path.join(js, 'toasts.js'), 'utf8');

function between(source, startMarker, endMarker) {
    const start = source.indexOf(startMarker);
    assert.notEqual(start, -1, startMarker + ' must exist');
    const end = source.indexOf(endMarker, start);
    assert.notEqual(end, -1, 'unterminated ' + startMarker);
    return source.slice(start, end + endMarker.length);
}

function load(reply) {
    const calls = [];
    const copied = [];
    const toasts = [];
    const RS = {
        copyText(text) { copied.push(text); return Promise.resolve(true); },
        invoke(name, args) {
            calls.push([name, JSON.parse(JSON.stringify(args))]);
            return reply();
        }
    };
    const context = {
        window: { RS },
        RS,
        showToast(...args) { toasts.push(args); }
    };
    vm.runInNewContext(
        between(stateSource, 'window.RS.copySecret = function(value) {', '\n};\n') + '\n' +
            between(toastSource, 'function showSecretCopiedToast(', '\n}\n'),
        context,
        { filename: 'secret-copy.js' }
    );
    return { RS, calls, copied, toasts, context };
}

(async () => {
    const copy = load(() => Promise.resolve(null));
    assert.equal(await copy.RS.copySecret('twelve words'), true);
    assert.deepEqual(copy.calls, [['copy_secret', { text: 'twelve words' }]]);
    assert.deepEqual(copy.copied, [], 'never through the WebView');

    const refused = load(() => Promise.reject({ code: 'unavailable', message: 'no clipboard' }));
    assert.equal(await refused.RS.copySecret('twelve words'), false, 'a failed copy says so');
    assert.deepEqual(refused.copied, [], 'and never falls back to a copy that stays');

    copy.context.showSecretCopiedToast('Recovery phrase');
    assert.deepEqual(copy.toasts, [
        ['Recovery phrase copied. It clears from the clipboard in a minute.', 'toast-success', 3000]
    ]);

    console.log('Secret copy tests passed');
})().catch(error => {
    console.error(error);
    process.exit(1);
});
