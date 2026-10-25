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
var lxmf = fs.readFileSync(path.join(root, 'dashboard/static/js/lxmf.js'), 'utf8');
var setup = fs.readFileSync(path.join(root, 'dashboard/static/js/setup.js'), 'utf8');

function block(source, start, what) {
    assert(start !== -1, 'missing ' + what);
    var depth = 0;
    for (var i = source.indexOf('{', start); i < source.length; i++) {
        if (source[i] === '{') depth++;
        else if (source[i] === '}' && --depth === 0) return source.slice(start, i + 1);
    }
    throw new Error('unterminated ' + what);
}

function functionSource(name, source) {
    source = source || identity;
    return block(source, source.indexOf('function ' + name + '('), name);
}

function listenerSource(event, source) {
    source = source || identity;
    var start = source.indexOf("RS.listen('" + event + "', function(");
    var body = block(source, start, 'listener ' + event);
    return body + source.slice(start + body.length, source.indexOf(';', start + body.length) + 1);
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
    setTimeout: function() {},
    showSetupConnectingStep: function() {},
    resetSetupToStart: function() { calls.push('resetSetupToStart'); },
    requestSetupNotificationPermissionIfEnabled: function() {},
    runConnectingProgress: function() {},
    normalizeContactList: function(data) { return data.slice(); },
    _refreshRenderedConversationNames: function() {},
    renderVoiceUi: function() {},
};
vm.createContext(page);
var pendingDeclaration = identity.slice(identity.indexOf('var _switchRequestsPending'),
    identity.indexOf(';', identity.indexOf('var _switchRequestsPending')) + 1);
assert(pendingDeclaration, 'missing the pending-switch count');
vm.runInContext(pendingDeclaration + '\n' + functionSource('switchToIdentity') + '\n' +
    functionSource('resetConversationCaches') + '\n' +
    listenerSource('identity_switching') + '\n' +
    listenerSource('identity_switched') + '\n' +
    listenerSource('identity_switch_failed') + '\n' +
    listenerSource('contacts_update', lxmf) + '\n' +
    functionSource('completeSetupAfterHardwareIdentity', setup), page, { filename: 'identity-switch.js' });
page.window.RS = page.RS;
page.RS.diag = function() {};

(async function() {
    var target = 'bb'.repeat(16);
    var reason = "Couldn't load this identity's key.";

    // The previous identity's contacts leave the window when a switch
    // starts, not when it ends.
    page.lxmfContacts = [{ hash: 'cc'.repeat(16), display_name: 'Previous' }];
    var rendered = 0;
    page.renderStandaloneContactList = function() { rendered += 1; };
    handlers.identity_switching({ hash: target, generation: '8' });
    assert.strictEqual(page.lxmfContacts.length, 0, 'the previous contacts stay while switching');
    assert.strictEqual(rendered, 1, 'the contact list is not redrawn');
    // The next identity's start sends its contacts before identity_switched,
    // which must keep them.
    var next = 'dd'.repeat(16);
    handlers.contacts_update([{ hash: next, display_name: 'Next' }]);
    handlers.identity_switched({ hash: target, lxmf_hash: '', display_name: 'B' });
    assert.strictEqual(page.lxmfContacts.length, 1, 'identity_switched dropped the new contacts');
    assert.strictEqual(page.lxmfContacts[0].hash, next);
    page.window._identitySwitchInProgress = false;
    toasts = [];
    calls = [];

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

    // Setup's hardware activation fails after the previous runtime stopped:
    // setup says why, once, and the closing event does not say it again.
    toasts = [];
    calls = [];
    var hardware = 'cc'.repeat(16);
    var activation = 'Could not activate hardware identity.';
    page.completeSetupAfterHardwareIdentity({ hash: hardware }, '123456');
    handlers.identity_switch_failed({ hash: hardware, generation: '11', error: activation });
    var refusedActivation = new Error(activation);
    refusedActivation.code = 'internal_error';
    fail(refusedActivation);
    await settle();
    assert.deepStrictEqual(toasts, [[activation, 'toast-error']], 'the failure is said twice');
    assert.strictEqual(page._switchRequestsPending, 0, 'the activation stays pending');
    console.log('Identity switch tests passed');
})().catch(function(error) {
    console.error(error);
    process.exit(1);
});
