#!/usr/bin/env node
'use strict';

var assert = require('assert');
var fs = require('fs');
var path = require('path');
var vm = require('vm');

var root = path.join(__dirname, '..', '..');
function read(file) {
    return fs.readFileSync(path.join(root, file), 'utf8');
}
var settings = read('dashboard/static/js/settings.js');
var identity = read('dashboard/static/js/identity.js');
var lxmf = read('dashboard/static/js/lxmf.js');

function functionSource(source, name) {
    var start = source.indexOf('function ' + name + '(');
    assert(start !== -1, 'missing ' + name);
    var depth = 0;
    for (var i = source.indexOf('{', start); i < source.length; i++) {
        if (source[i] === '{') depth++;
        else if (source[i] === '}' && --depth === 0) return source.slice(start, i + 1);
    }
    throw new Error('unterminated ' + name);
}

function settle() {
    return new Promise(function(resolve) { setImmediate(resolve); });
}

// Both confirmations of each clear say that blocks survive it.
[
    'Delete ALL contacts? Blocked users stay blocked. This cannot be undone.',
    'Clear ALL messages and contacts? Blocked users stay blocked. This cannot be undone.',
].forEach(function(copy) {
    assert.equal(settings.split("'" + copy + "'").length - 1, 2, copy);
});

// An identity switch and a bulk delete drop the same conversation caches.
var switched = identity.slice(
    identity.indexOf("RS.listen('identity_switched'"),
    identity.indexOf('// Hardware Key (YubiKey/Nitrokey PIV) identity flow')
);
assert(switched.includes('resetConversationCaches()'));
assert(!switched.includes('_conversationCache'), 'the switch keeps no copy of the reset');

var caches = { RS: { text: { utf8Length: function(value) { return Buffer.byteLength(String(value)); } } } };
vm.createContext(caches);
vm.runInContext(
    lxmf.slice(0, lxmf.indexOf('var _imageBlobUrlCache')) +
        ['_canonicalConversationHash', '_utf8ByteLength', 'cacheGet', 'cacheSet', 'cacheDel',
            '_mergeConversationMessages'].map(function(name) {
            return functionSource(lxmf, name);
        }).join('\n') + '\n' + functionSource(identity, 'resetConversationCaches'),
    caches,
    { filename: 'conversation-caches.js' }
);
var peer = 'aa'.repeat(16);
var recentSend = { id: 'sent-1', direction: 'outbound', state: 'sent', timestamp: Date.now() / 1000 };
caches.cacheSet(peer, [recentSend]);
caches.lxmfConversation = [recentSend];
assert.equal(caches._mergeConversationMessages(peer, []).length, 1,
    'a cached recent send survives an empty fetch');
caches.resetConversationCaches();
assert.equal(caches._mergeConversationMessages(peer, []).length, 0,
    'a deleted recent send must not come back when its conversation reopens');
assert.equal(caches.lxmfConversation.length, 0);
assert.equal(caches._conversationCacheBytes, 0);

function listenerSource(source, event) {
    var marker = "RS.listen('" + event + "', function(";
    var start = source.indexOf(marker);
    assert(start !== -1, 'missing listener ' + event);
    var depth = 0;
    for (var i = source.indexOf('{', start); i < source.length; i++) {
        if (source[i] === '{') depth++;
        else if (source[i] === '}' && --depth === 0) return source.slice(start, source.indexOf(';', i) + 1);
    }
    throw new Error('unterminated listener ' + event);
}

// The backend announces a bulk delete before it re-sends the lists, and
// events arrive in order: the window resets on that event, so nothing that
// arrives after it is wiped and no earlier list survives it.
var calls = [];
var handlers = {};
var page = {
    RS: { listen: function(event, handler) { handlers[event] = handler; } },
    lxmfContacts: [{ hash: peer }],
    resetConversationCaches: function() { calls.push('reset'); },
    clearReplyTarget: function() { calls.push('reply'); },
    _resetConversationList: function() { calls.push('list'); },
    renderStandaloneContactList: function() { calls.push('contacts'); },
    renderConversation: function() { calls.push('render'); },
    _reloadPeersAfterCacheClear: function(keepOnFailure) {
        calls.push(keepOnFailure ? 'peers (kept on failure)' : 'peers');
    },
};
vm.createContext(page);
vm.runInContext(listenerSource(identity, 'identity_data_cleared'), page,
    { filename: 'identity-data-cleared.js' });
[
    [{ messages: true, contacts: false },
        ['reset', 'reply', 'list', 'render', 'peers (kept on failure)'], 1],
    [{ messages: false, contacts: true },
        ['contacts', 'render', 'peers (kept on failure)'], 0],
    [{ messages: true, contacts: true },
        ['reset', 'reply', 'list', 'contacts', 'render', 'peers (kept on failure)'], 0],
].forEach(function(entry) {
    calls = [];
    page.lxmfContacts = [{ hash: peer }];
    handlers.identity_data_cleared(entry[0]);
    assert.deepEqual(calls, entry[1], JSON.stringify(entry[0]));
    assert.equal(page.lxmfContacts.length, entry[2],
        'a contacts clear empties the list the Add-contact button reads');
});

// A failed peer reload after a bulk delete keeps the peers the window has.
var events = read('dashboard/static/js/tauri_events.js');
var peers = [];
var reload = {
    PeersCache: {
        replace: function() { peers.push('replace'); },
        clear: function() { peers.push('clear'); },
    },
    RS: { invoke: function() { return Promise.reject(new Error('offline')); } },
};
vm.createContext(reload);
vm.runInContext(functionSource(events, '_reloadPeersAfterCacheClear'), reload,
    { filename: 'peers-reload.js' });

// The commands' own replies drive nothing: the event already reset the window.
var replies = [];
var danger = {
    rsConfirm: function() { return Promise.resolve(true); },
    RS: { invoke: function(name) { replies.push('invoke ' + name); return Promise.resolve(null); } },
    showToast: function() {},
    haptic: function() {},
    closeDangerZone: function() {},
    resetConversationCaches: function() { replies.push('reset'); },
    renderConversation: function() { replies.push('render'); },
    _reloadPeersAfterCacheClear: function() { replies.push('peers'); },
};
vm.createContext(danger);
vm.runInContext(
    ['clearWithConfirm', 'confirmDangerAction'].map(function(name) {
        return functionSource(settings, name);
    }).join('\n'),
    danger,
    { filename: 'danger-zone.js' }
);

(async function() {
    reload._reloadPeersAfterCacheClear(true);
    await settle();
    assert.deepEqual(peers, [], 'kept on failure');
    reload._reloadPeersAfterCacheClear();
    await settle();
    assert.deepEqual(peers, ['clear'], 'a cleared announce cache still drops its peers');

    var cases = [
        ['clearWithConfirm', 'api_clear_messages'],
        ['clearWithConfirm', 'api_clear_contacts'],
        ['clearWithConfirm', 'api_reset_database'],
        ['confirmDangerAction', 'clear-messages'],
        ['confirmDangerAction', 'clear-contacts'],
        ['confirmDangerAction', 'clear-all-data'],
    ];
    for (var i = 0; i < cases.length; i++) {
        replies = [];
        if (cases[i][0] === 'clearWithConfirm') {
            danger.clearWithConfirm(cases[i][1], 'Sure?', 'Done', 'Failed');
        } else {
            danger.confirmDangerAction(cases[i][1]);
        }
        await settle();
        await settle();
        assert.equal(replies.length, 1, cases[i][1] + ': ' + replies.join(', '));
        assert.equal(replies[0].indexOf('invoke '), 0, cases[i][1]);
    }
    console.log('Bulk delete tests passed');
})().catch(function(error) {
    console.error(error);
    process.exit(1);
});
