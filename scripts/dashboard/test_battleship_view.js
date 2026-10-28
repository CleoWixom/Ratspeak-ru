#!/usr/bin/env node
'use strict';

var assert = require('assert');
var fs = require('fs');
var path = require('path');
var vm = require('vm');

var jsRoot = path.join(__dirname, '..', '..', 'dashboard', 'static', 'js');
var registrySource = fs.readFileSync(path.join(jsRoot, 'game_registry.js'), 'utf8');
var viewSource = fs.readFileSync(path.join(jsRoot, 'battleship_view.js'), 'utf8');

var windowStub = {
    RS: {},
    document: { documentElement: {} },
    getComputedStyle: function() { return { getPropertyValue: function() { return ''; } }; },
};
var context = {
    window: windowStub, document: windowStub.document, Object: Object, TypeError: TypeError,
    Error: Error, Date: Date, Math: Math, RegExp: RegExp, Array: Array, String: String,
    parseInt: parseInt, Number: Number,
};
vm.createContext(context);
vm.runInContext(registrySource, context, { filename: 'game_registry.js' });
vm.runInContext(viewSource, context, { filename: 'battleship_view.js' });

var adapter = windowStub.RS.games.views.get('battleship');
assert(adapter, 'Battleship adapter must register under its canonical app_id');
assert.strictEqual(adapter.boardSelector, '.bs-wrap');
assert.deepStrictEqual(Array.from(adapter.actions), ['challenge', 'accept', 'decline', 'move', 'resign']);
assert(!Array.from(adapter.actions).some(function(a) { return a.indexOf('draw') === 0; }),
    'Battleship has no draws');

var WATER100 = new Array(101).join('_');
function session(overrides) {
    var base = {
        game_id: 'bs-test', app_id: 'battleship', status: 'active', contact_hash: 'them',
        challenger: 'me', phase: 'setup', turn: '', move_count: 0, my_commit: '', their_commit: '',
        my_board: '', shots_in: WATER100, shots_out: WATER100, their_board: '', my_sunk: [],
        their_sunk: [], owes_reveal: false, winner: '', terminal: '', cheat_reason: '',
        last_in: null, last_out: null,
    };
    Object.keys(overrides || {}).forEach(function(key) { base[key] = overrides[key]; });
    return base;
}
function ctx(extra) {
    var sent = [];
    var base = {
        root: null, isMe: function(hash) { return hash === 'me'; },
        contactName: function() { return 'Opp'; },
        sendMove: function(payload, optimistic) { sent.push({ payload: payload, optimistic: optimistic }); return true; },
    };
    Object.keys(extra || {}).forEach(function(key) { base[key] = extra[key]; });
    base.sent = sent;
    return base;
}
function fleetBoard() {
    // Horizontal ships on rows 0..4: ids 0 (5), 1 (4), 2 (3), 3 (3), 4 (2).
    var cells = new Array(100).fill('_');
    [[0, 5, 0], [10, 4, 1], [20, 3, 2], [30, 3, 3], [40, 2, 4]].forEach(function(s) {
        for (var i = 0; i < s[1]; i++) cells[s[0] + i] = String(s[2]);
    });
    return cells.join('');
}

// ---- placement ----
var html = adapter.renderBoard(session(), ctx());
assert(html.includes('bs-placement'), 'setup shows the placement panel');
assert(html.includes('data-action="ready" disabled'), 'Ready is disabled until the fleet is complete');
assert(html.includes('Tap the grid to place the Carrier (5 cells)'));
assert.strictEqual((html.match(/class="bs-cell bs-water bs-place"/g) || []).length, 100);

function fakeRoot() {
    var handlers = {};
    var node = {
        innerHTML: '', handlers: handlers,
        addEventListener: function(name, fn) { handlers[name] = fn; },
        contains: function() { return true; },
        querySelector: function() { return null; },
        querySelectorAll: function() { return []; },
    };
    return {
        node: node,
        querySelector: function(selector) { return selector === '.bs-placement' ? node : null; },
        querySelectorAll: function() { return []; },
    };
}
function click(node, attrs) {
    var button = {
        hasAttribute: function(name) { return Object.prototype.hasOwnProperty.call(attrs, name); },
        getAttribute: function(name) { return attrs[name]; },
        classList: { add: function() {} }, disabled: false,
    };
    node.handlers.click({ target: { closest: function() { return button; } } });
    return button;
}

var placing = session({ game_id: 'bs-place' });
var root = fakeRoot();
var c = ctx({ root: root });
adapter.bindBoard(placing, c);
assert(root.node.handlers.click, 'placement binds a delegated click handler');
click(root.node, { 'data-action': 'random' });
assert(!root.node.innerHTML.includes('data-action="ready" disabled'), 'Random completes the fleet');
var ready = click(root.node, { 'data-action': 'ready' });
assert.strictEqual(ready.disabled, true);
assert.strictEqual(c.sent.length, 1);
var intent = c.sent[0].payload;
assert.strictEqual(intent.k, 'c');
assert.strictEqual(intent.p.length, 5);
assert(intent.p.every(function(e) { return Number.isInteger(e) && e >= 0 && e < 200; }));
// The optimistic update only marks the commitment as pending; the fleet stays local.
var live = session({ game_id: 'bs-place' });
c.sent[0].optimistic.apply(live);
assert.strictEqual(live.my_commit, 'pending');
assert.strictEqual(live.my_board.length, 100);

// Manual placement: invalid spots are refused, valid ones advance the selection.
root = fakeRoot();
adapter.bindBoard(session({ game_id: 'bs-manual' }), ctx({ root: root }));
click(root.node, { 'data-cell': '8' });          // carrier horizontal at column 8 overflows
assert(root.node.innerHTML === '', 'an illegal placement does not re-render the fleet');
click(root.node, { 'data-cell': '20' });         // carrier on cells 20-24
assert(root.node.innerHTML.includes('Warship (4 cells)'), 'next unplaced ship is selected');
click(root.node, { 'data-action': 'rotate' });
var before = root.node.innerHTML;
assert(before.includes('Rotate: vertical'));
var refused = click(root.node, { 'data-cell': '4' }); // 4,14,24,34 crosses the carrier at 24
assert.strictEqual(root.node.innerHTML, before, 'an overlapping placement is refused');
click(root.node, { 'data-cell': '5' });          // 5,15,25,35 is free
assert(root.node.innerHTML.includes('Cruiser (3 cells)'));
click(root.node, { 'data-cell': '22' });         // tapping a placed ship picks it up
assert(root.node.innerHTML.includes('Carrier (5 cells)'));

// ---- battle ----
var battle = session({
    phase: 'battle', turn: 'me', my_commit: 'aa', their_commit: 'bb', my_board: fleetBoard(),
    shots_out: 'h' + 'm' + new Array(99).join('_') , last_out: 1,
});
assert.strictEqual(battle.shots_out.length, 100);
html = adapter.renderBoard(battle, ctx());
assert((html.match(/bs-fire/g) || []).length === 98, 'only unfired enemy cells are fireable');
assert(html.includes('bs-hit') && html.includes('bs-miss') && html.includes('bs-last'));
assert(html.includes('Enemy fleet') && html.includes('Your fleet'));
assert(html.includes('Opp\u2019s waters'));

var fireRoot = {
    querySelectorAll: function(selector) {
        if (selector !== '.bs-fire') return [];
        return [{
            getAttribute: function() { return '57'; }, focus: function() {},
            addEventListener: function(name, fn) { if (name === 'click') this.click = fn; },
        }];
    },
    querySelector: function() { return null; },
};
var fireCtx = ctx({ root: fireRoot });
var captured = [];
fireRoot.querySelectorAll = (function(original) {
    return function(selector) {
        var list = original(selector);
        captured = list.length ? list : captured;
        return list;
    };
})(fireRoot.querySelectorAll);
adapter.bindBoard(battle, fireCtx);
captured[0].click();
assert.strictEqual(JSON.stringify(fireCtx.sent[0].payload), JSON.stringify({ k: 'f', c: 57 }));
var optimistic = battle.shots_out && Object.assign({}, battle);
fireCtx.sent[0].optimistic.apply(optimistic);
assert.strictEqual(optimistic.shots_out.charAt(57), 'p');
assert.strictEqual(optimistic.turn, 'them');

// Not my turn: nothing is fireable and nothing is bound.
html = adapter.renderBoard(session({ phase: 'battle', turn: 'them', my_commit: 'aa', their_commit: 'bb',
    my_board: fleetBoard() }), ctx());
assert(!html.includes('bs-fire'));

// ---- status text and chips ----
assert.strictEqual(adapter.activeStatusText(session(), ctx()), 'Place your fleet');
assert.strictEqual(adapter.activeStatusText(session({ my_commit: 'aa' }), ctx()), 'Waiting for Opp\u2019s fleet');
assert.strictEqual(adapter.activeStatusText(battle, ctx()), 'Your turn \u00b7 fire!');
assert.strictEqual(adapter.activeStatusText(session({ phase: 'battle', turn: 'them' }), ctx()), 'Opp\u2019s turn');
assert.strictEqual(adapter.activeStatusText(session({ phase: 'reveal', owes_reveal: true }), ctx()), 'Revealing your fleet…');
assert.strictEqual(adapter.activeStatusText(session({ phase: 'reveal' }), ctx()), 'Verifying the fleet…');
assert.deepStrictEqual(Array.from(adapter.detailChips(battle, ctx())), ['Hits 1/17', 'Sunk 0/5']);
assert.deepStrictEqual(Array.from(adapter.detailChips(session({ status: 'pending' }), ctx())), []);

// ---- automatic, retried-but-throttled reveal ----
var revealCtx = ctx();
var owing = session({ game_id: 'bs-reveal', phase: 'reveal', owes_reveal: true });
adapter.onSessionDelta(owing, null, revealCtx);
adapter.onSessionDelta(owing, null, revealCtx);
assert.strictEqual(revealCtx.sent.length, 1, 'the reveal is not spammed');
assert.strictEqual(JSON.stringify(revealCtx.sent[0].payload), JSON.stringify({ k: 'r' }));
adapter.onSessionDelta(session({ game_id: 'bs-reveal', phase: 'reveal', owes_reveal: false }), null, revealCtx);
adapter.onSessionDelta(owing, null, revealCtx);
assert.strictEqual(revealCtx.sent.length, 2, 'a later reveal obligation sends again');
adapter.onSessionDelta(session({ game_id: 'bs-x', status: 'completed', phase: 'done' }), null, revealCtx);
assert.strictEqual(revealCtx.sent.length, 2, 'completed games never reveal');

// ---- results ----
function result(overrides) {
    return adapter.renderBoard(session(Object.assign({ status: 'completed', phase: 'done',
        my_board: fleetBoard(), their_board: fleetBoard() }, overrides)), ctx());
}
assert(result({ terminal: 'win', winner: 'me' }).includes('You won!'));
assert(result({ terminal: 'win', winner: 'them' }).includes('You lost'));
assert(result({ terminal: 'resign', winner: 'me' }).includes('Opponent resigned'));
var cheat = result({ terminal: 'cheat', winner: 'me', cheat_reason: 'reported more than 17 hits' });
assert(cheat.includes('not honest') && cheat.includes('reported more than 17 hits'));
assert(result({ terminal: 'win', winner: 'me' }).includes('bs-revealed'), 'the loser fleet is shown after reveal');

// ---- escaping ----
adapter.renderBoard(session({ game_id: '"><img src=x>' }), ctx()).split('"><img').length === 1 || assert.fail('unescaped id');
assert(!adapter.renderBoard(session({ game_id: '"><script>' }), ctx()).includes('"><script>'));

console.log('battleship view tests passed (placement, fire, status, auto-reveal, results)');
