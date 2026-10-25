#!/usr/bin/env node
'use strict';

// A switch that does not finish ends with identity_switch_failed: the window
// leaves its switching state, whatever the generation, and says why.

var assert = require('assert');
var fs = require('fs');
var path = require('path');
var vm = require('vm');

var root = path.join(__dirname, '..', '..');
var identity = fs.readFileSync(path.join(root, 'dashboard/static/js/identity.js'), 'utf8');

function block(source, start, what) {
    assert(start !== -1, 'missing ' + what);
    var depth = 0;
    for (var i = source.indexOf('{', start); i < source.length; i++) {
        if (source[i] === '{') depth++;
        else if (source[i] === '}' && --depth === 0) return source.slice(start, i + 1);
    }
    throw new Error('unterminated ' + what);
}

function functionSource(name) {
    return block(identity, identity.indexOf('function ' + name + '('), name);
}

function listenerSource(event) {
    var start = identity.indexOf("RS.listen('" + event + "', function(");
    var body = block(identity, start, 'listener ' + event);
    return body + identity.slice(start + body.length, identity.indexOf(';', start + body.length) + 1);
}

function settle() {
    return new Promise(function(resolve) { setImmediate(resolve); });
}

var handlers = {};
var calls = [];
var toasts = [];
var fail = null;
var card = { innerHTML: '' };
var page = {
    window: {},
    document: {
        getElementById: function(id) { return id === 'identity-active-card' ? card : null; },
    },
    RS: {
        listen: function(event, handler) { handlers[event] = handler; },
        invoke: function(name) {
            calls.push(name);
            return new Promise(function(_resolve, reject) { fail = reject; });
        },
    },
    showToast: function(message, kind) { toasts.push([message, kind]); },
    loadIdentities: function() { calls.push('loadIdentities'); },
    loadConversations: function() { calls.push('loadConversations'); },
};
vm.createContext(page);
var pendingDeclaration = identity.slice(identity.indexOf('var _switchRequestsPending'),
    identity.indexOf(';', identity.indexOf('var _switchRequestsPending')) + 1);
assert(pendingDeclaration, 'missing the pending-switch count');
vm.runInContext(pendingDeclaration + '\n' + functionSource('switchToIdentity') + '\n' +
    listenerSource('identity_switch_failed'), page, { filename: 'identity-switch.js' });

(async function() {
    var target = 'bb'.repeat(16);
    var reason = "Couldn't load this identity's key.";

    // The switch began and failed: its closing event ends it.
    page.switchToIdentity(target);
    assert(card.innerHTML.indexOf('Switching identity') !== -1);
    page.window._identitySwitchInProgress = true;
    calls = [];
    handlers.identity_switch_failed({ hash: target, generation: '9', error: reason });
    assert.strictEqual(page.window._identitySwitchInProgress, false, 'the window stays mid-switch');
    assert(calls.indexOf('loadIdentities') !== -1, 'the switching overlay stays');
    assert(calls.indexOf('loadConversations') !== -1, 'the restored identity is not reloaded');
    var error = new Error(reason);
    error.code = 'bad_request';
    fail(error);
    await settle();
    assert.deepStrictEqual(toasts, [[reason, 'toast-error']], 'the reason is swallowed');

    // Started elsewhere, by a removal's re-activation or another window: the
    // event is all this window hears, so it says why.
    toasts = [];
    calls = [];
    handlers.identity_switch_failed({ hash: target, generation: '10', error: reason });
    assert.deepStrictEqual(toasts, [[reason, 'toast-error']], 'a switch this window did not start fails silently');

    // Refused before it began: no event, so the reply clears the overlay.
    toasts = [];
    calls = [];
    page.switchToIdentity(target);
    var refused = new Error('Identity not found');
    refused.code = 'not_found';
    fail(refused);
    await settle();
    assert.deepStrictEqual(toasts, [['Identity not found', 'toast-error']]);
    assert(calls.indexOf('loadIdentities') !== -1, 'a refused switch leaves its overlay');
    console.log('Identity switch tests passed');
})().catch(function(error) {
    console.error(error);
    process.exit(1);
});
