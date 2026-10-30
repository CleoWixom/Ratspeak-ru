// Ratspeak-ru: Battleship view. Fork-only file; loaded by ru_loader.js and registered
// through the same RS.games.views registry the upstream games use. Engine: crates/ratspeak-runtime/src/battleship.rs.
(function(global) {
    'use strict';

    var RS = global.RS = global.RS || {};
    if (!RS.games || !RS.games.views) {
        throw new Error('Game view registry must load before ru_battleship.js');
    }

    var APP_ID = 'ru_battleship';
    var GRID = 10;
    var CELLS = GRID * GRID;
    var SHIP_CELLS = 17;
    var SHIPS = [
        { name: 'Carrier', length: 5 },
        { name: 'Warship', length: 4 },
        { name: 'Cruiser', length: 3 },
        { name: 'Submarine', length: 3 },
        { name: 'Destroyer', length: 2 },
    ];
    var COLUMN_LETTERS = 'ABCDEFGHIJ';
    var REVEAL_RETRY_MS = 15000;

    // Local, never-transmitted placement drafts keyed by session id, and the
    // time of the last automatic reveal attempt (the reveal is protocol-
    // mandatory, so the view sends it without waiting for a tap).
    var _drafts = Object.create(null);
    var _revealSentAt = Object.create(null);

    function _meta(session, key, fallback) {
        return RS.games.state.value(session, key, fallback);
    }

    function _escape(value) {
        return String(value == null ? '' : value)
            .replace(/&/g, '&amp;')
            .replace(/</g, '&lt;')
            .replace(/>/g, '&gt;')
            .replace(/"/g, '&quot;')
            .replace(/'/g, '&#39;');
    }

    function _grid(session, key, allowed) {
        var value = _meta(session, key, '');
        if (typeof value !== 'string' || value.length !== CELLS) return '';
        var pattern = new RegExp('[^' + allowed + ']');
        return pattern.test(value) ? '' : value;
    }

    function _list(session, key) {
        var value = _meta(session, key, []);
        return Array.isArray(value) ? value : [];
    }

    function _coord(cell) {
        return COLUMN_LETTERS.charAt(cell % GRID) + (Math.floor(cell / GRID) + 1);
    }

    function _commitState(session) {
        var commit = _meta(session, 'my_commit', '');
        return typeof commit === 'string' ? commit : '';
    }

    function _phase(session) {
        var phase = _meta(session, 'phase', 'setup');
        return typeof phase === 'string' ? phase : 'setup';
    }

    function _isMyTurn(session, context) {
        return session.status === 'active' && _phase(session) === 'battle' &&
            !!(context && context.isMe && context.isMe(_meta(session, 'turn', '')));
    }

    function _hitsOut(session) {
        var shots = _grid(session, 'shots_out', '_pmh');
        var hits = 0;
        for (var i = 0; i < shots.length; i++) if (shots.charAt(i) === 'h') hits++;
        return hits;
    }

    // ---- fleet geometry (mirrors the Rust rules; the backend re-validates) ----

    function _shipCells(index, start, vertical) {
        var length = SHIPS[index].length;
        var row = Math.floor(start / GRID);
        var column = start % GRID;
        if (start < 0 || start >= CELLS) return null;
        if (vertical ? row + length > GRID : column + length > GRID) return null;
        var cells = [];
        for (var step = 0; step < length; step++) {
            cells.push(vertical ? start + step * GRID : start + step);
        }
        return cells;
    }

    function _decodeShip(index, encoded) {
        if (typeof encoded !== 'number') return null;
        return _shipCells(index, Math.floor(encoded / 2), encoded % 2 === 1);
    }

    function _occupancy(fleet, skipIndex) {
        var board = [];
        for (var i = 0; i < CELLS; i++) board.push(-1);
        for (var ship = 0; ship < SHIPS.length; ship++) {
            if (ship === skipIndex) continue;
            var cells = _decodeShip(ship, fleet[ship]);
            if (!cells) continue;
            for (var c = 0; c < cells.length; c++) board[cells[c]] = ship;
        }
        return board;
    }

    function _tryPlace(fleet, index, start, vertical) {
        var cells = _shipCells(index, start, vertical);
        if (!cells) return null;
        var occupied = _occupancy(fleet, index);
        for (var i = 0; i < cells.length; i++) if (occupied[cells[i]] !== -1) return null;
        var next = fleet.slice();
        next[index] = start * 2 + (vertical ? 1 : 0);
        return next;
    }

    function _randomFleet() {
        for (var attempt = 0; attempt < 200; attempt++) {
            var fleet = [null, null, null, null, null];
            var ok = true;
            for (var ship = 0; ship < SHIPS.length && ok; ship++) {
                var placed = null;
                for (var tries = 0; tries < 100 && !placed; tries++) {
                    placed = _tryPlace(fleet, ship, Math.floor(Math.random() * CELLS),
                        Math.random() < 0.5);
                }
                if (placed) fleet = placed; else ok = false;
            }
            if (ok) return fleet;
        }
        return null;
    }

    function _fleetComplete(fleet) {
        for (var i = 0; i < SHIPS.length; i++) if (typeof fleet[i] !== 'number') return false;
        return true;
    }

    function _boardString(fleet) {
        var occupied = _occupancy(fleet, -1);
        var text = '';
        for (var i = 0; i < CELLS; i++) text += occupied[i] === -1 ? '_' : String(occupied[i]);
        return text;
    }

    // ---- rendering ----

    function _gridHtml(label, cellHtml) {
        var html = '<div class="bs-grid" role="grid" aria-label="' + _escape(label) + '">';
        html += '<span class="bs-coord bs-corner" aria-hidden="true"></span>';
        for (var c = 0; c < GRID; c++) {
            html += '<span class="bs-coord" aria-hidden="true">' + COLUMN_LETTERS.charAt(c) + '</span>';
        }
        for (var cell = 0; cell < CELLS; cell++) {
            if (cell % GRID === 0) {
                html += '<span class="bs-coord" aria-hidden="true">' + (cell / GRID + 1) + '</span>';
            }
            html += cellHtml(cell);
        }
        return html + '</div>';
    }

    function _staticCell(classes, cell, state) {
        return '<div role="gridcell" class="' + classes.join(' ') + '" data-cell="' + cell +
            '" aria-label="' + _escape(_coord(cell) + ': ' + state) + '"></div>';
    }

    function _enemyCellHtml(session, fireable) {
        var shots = _grid(session, 'shots_out', '_pmhp') || new Array(CELLS + 1).join('_');
        var revealed = _grid(session, 'their_board', '_01234');
        var last = parseInt(_meta(session, 'last_out', -1), 10);
        return function(cell) {
            var shot = shots.charAt(cell);
            var classes = ['bs-cell', 'bs-water'];
            var state = 'unknown';
            if (shot === 'h') { classes.push('bs-hit'); state = 'hit'; }
            else if (shot === 'm') { classes.push('bs-miss'); state = 'miss'; }
            else if (shot === 'p') { classes.push('bs-pending'); state = 'awaiting answer'; }
            else if (revealed && revealed.charAt(cell) !== '_') {
                classes.push('bs-ship', 'bs-revealed');
                state = 'enemy ship (revealed)';
            }
            if (cell === last) classes.push('bs-last');
            if (fireable && shot === '_') {
                classes.push('bs-fire');
                return '<button type="button" role="gridcell" class="' + classes.join(' ') +
                    '" data-cell="' + cell + '" aria-label="Fire at ' + _coord(cell) + '"></button>';
            }
            return _staticCell(classes, cell, state);
        };
    }

    function _ownCellHtml(session) {
        var fleet = _grid(session, 'my_board', '_01234');
        var shots = _grid(session, 'shots_in', '_mh') || new Array(CELLS + 1).join('_');
        var sunk = _list(session, 'my_sunk');
        var last = parseInt(_meta(session, 'last_in', -1), 10);
        return function(cell) {
            var ship = fleet ? fleet.charAt(cell) : '_';
            var shot = shots.charAt(cell);
            var classes = ['bs-cell', 'bs-water'];
            var state = 'water';
            if (ship !== '_') {
                classes.push('bs-ship');
                state = 'your ship';
                if (sunk.indexOf(parseInt(ship, 10)) !== -1) classes.push('bs-sunk');
            }
            if (shot === 'h') { classes.push('bs-hit'); state = 'your ship, hit'; }
            else if (shot === 'm') { classes.push('bs-miss'); state = 'enemy miss'; }
            if (cell === last) classes.push('bs-last');
            return _staticCell(classes, cell, state);
        };
    }

    function _fleetRow(label, sunk) {
        var html = '<div class="bs-fleet-row" role="group" aria-label="' + _escape(label) + '">' +
            '<span class="bs-fleet-label">' + _escape(label) + '</span>';
        for (var i = 0; i < SHIPS.length; i++) {
            var down = sunk.indexOf(i) !== -1;
            html += '<span class="bs-ship-chip bs-ship-chip-static' + (down ? ' sunk' : '') + '" ' +
                'title="' + _escape(SHIPS[i].name) + '">' +
                _escape(SHIPS[i].name.charAt(0)) + SHIPS[i].length +
                '<span class="bs-visually-hidden">' + _escape(SHIPS[i].name + (down ? ', sunk' : ', afloat')) +
                '</span></span>';
        }
        return html + '</div>';
    }

    function _draftFor(session) {
        var id = session.game_id;
        if (!_drafts[id]) _drafts[id] = { fleet: [null, null, null, null, null], selected: 0, vertical: false };
        return _drafts[id];
    }

    function _placementInner(session) {
        var draft = _draftFor(session);
        var occupied = _occupancy(draft.fleet, -1);
        var complete = _fleetComplete(draft.fleet);
        var html = '<p class="bs-hint">' + (complete
            ? 'Fleet ready. Tap Ready to lock it in, or rearrange it first.'
            : 'Tap the grid to place the ' + _escape(SHIPS[draft.selected].name) + ' (' +
                SHIPS[draft.selected].length + ' cells). Tap a placed ship to pick it up.') + '</p>';
        html += _gridHtml('Your fleet placement', function(cell) {
            var ship = occupied[cell];
            var classes = ['bs-cell', 'bs-water', 'bs-place'];
            var state = 'empty';
            if (ship !== -1) { classes.push('bs-ship'); state = 'your ship'; }
            return '<button type="button" role="gridcell" class="' + classes.join(' ') +
                '" data-cell="' + cell + '" aria-label="' + _escape(_coord(cell) + ': ' + state) + '"></button>';
        });
        html += '<div class="bs-ship-picker" role="group" aria-label="Ships to place">';
        for (var i = 0; i < SHIPS.length; i++) {
            var placed = typeof draft.fleet[i] === 'number';
            html += '<button type="button" class="bs-ship-chip' + (i === draft.selected ? ' selected' : '') +
                (placed ? ' placed' : '') + '" data-ship="' + i + '" aria-pressed="' +
                (i === draft.selected ? 'true' : 'false') + '">' + _escape(SHIPS[i].name) + ' · ' +
                SHIPS[i].length + (placed ? ' ✓' : '') + '</button>';
        }
        html += '</div>';
        html += '<div class="bs-place-actions">' +
            '<button type="button" class="nr-btn nr-btn-secondary" data-action="rotate" aria-pressed="' +
                (draft.vertical ? 'true' : 'false') + '">Rotate: ' + (draft.vertical ? 'vertical' : 'horizontal') + '</button>' +
            '<button type="button" class="nr-btn nr-btn-secondary" data-action="random">Random</button>' +
            '<button type="button" class="nr-btn nr-btn-ghost" data-action="clear">Clear</button>' +
            '<button type="button" class="nr-btn games-ctrl-accept" data-action="ready"' +
                (complete ? '' : ' disabled') + '>Ready</button>' +
        '</div>';
        return html;
    }

    function _renderBoard(session, context) {
        var opponent = context && context.contactName
            ? context.contactName(session.contact_hash) : 'Opponent';
        var html = '<div class="bs-wrap">';
        var phase = _phase(session);
        var committed = _commitState(session) !== '';

        if (session.status === 'pending') {
            html += '<div class="bs-boards">' +
                '<section class="bs-panel">' + _gridHtml('Your fleet', _ownCellHtml(session)) + '</section>' +
            '</div>';
            var initiator = session.challenger || session.initiator || _meta(session, 'initiator', '');
            var incoming = !(context && context.isMe && context.isMe(initiator));
            html += '<div class="bs-banner">' +
                (incoming ? 'Challenge received!' : 'Waiting for response…') + '</div>';
        } else if (session.status === 'active' && phase === 'setup' && !committed) {
            html += '<section class="bs-placement bs-panel" data-session="' + _escape(session.game_id) + '">' +
                _placementInner(session) + '</section>';
        } else {
            var fireable = _isMyTurn(session, context);
            html += '<div class="bs-boards">';
            html += '<section class="bs-panel bs-panel-enemy' + (fireable ? ' your-turn' : '') + '">' +
                '<h3 class="bs-panel-title">' + _escape(opponent) + '\u2019s waters</h3>' +
                _gridHtml('Enemy waters', _enemyCellHtml(session, fireable)) +
                _fleetRow('Enemy fleet', _list(session, 'their_sunk')) +
            '</section>';
            html += '<section class="bs-panel bs-panel-mine">' +
                '<h3 class="bs-panel-title">Your fleet</h3>' +
                _gridHtml('Your fleet', _ownCellHtml(session)) +
                _fleetRow('Your fleet', _list(session, 'my_sunk')) +
            '</section>';
            html += '</div>';
            if (session.status === 'active' && phase === 'setup' && committed) {
                html += '<div class="bs-banner">Fleet locked in. Waiting for ' + _escape(opponent) +
                    ' to place theirs…</div>';
            }
        }

        if (session.status === 'completed') {
            var terminal = _meta(session, 'terminal', '');
            var won = !!(context && context.isMe && context.isMe(_meta(session, 'winner', '')));
            var result;
            if (terminal === 'resign') result = won ? 'Opponent resigned' : 'You resigned';
            else if (terminal === 'cheat') result = won ? 'You won — opponent\u2019s answers were not honest' : 'Game ended: fleet check failed';
            else result = won ? 'You won!' : 'You lost';
            html += '<div class="bs-result ' + (won ? 'won' : 'lost') + '" role="status">' + _escape(result) + '</div>';
            var reason = _meta(session, 'cheat_reason', '');
            if (terminal === 'cheat' && reason) {
                html += '<div class="bs-reason">' + _escape(reason) + '</div>';
            }
        }
        return html + '</div>';
    }

    // ---- interaction ----

    function _bindPlacement(session, context) {
        var root = context.root && context.root.querySelector
            ? context.root.querySelector('.bs-placement') : null;
        if (!root) return;
        var draft = _draftFor(session);

        function refresh() {
            root.innerHTML = _placementInner(session);
        }
        function nextUnplaced(from) {
            for (var step = 0; step < SHIPS.length; step++) {
                var index = (from + step) % SHIPS.length;
                if (typeof draft.fleet[index] !== 'number') return index;
            }
            return draft.selected;
        }
        function previewAt(cell) {
            clearPreview();
            var cells = _shipCells(draft.selected, cell, draft.vertical);
            if (!cells) return;
            var valid = !!_tryPlace(draft.fleet, draft.selected, cell, draft.vertical);
            for (var i = 0; i < cells.length; i++) {
                var el = root.querySelector('.bs-place[data-cell="' + cells[i] + '"]');
                if (el) el.classList.add(valid ? 'is-preview' : 'is-invalid');
            }
        }
        function clearPreview() {
            var marked = root.querySelectorAll('.is-preview, .is-invalid');
            for (var i = 0; i < marked.length; i++) {
                marked[i].classList.remove('is-preview', 'is-invalid');
            }
        }

        root.addEventListener('click', function(event) {
            var target = event.target.closest ? event.target.closest('button') : null;
            if (!target || !root.contains(target)) return;
            if (target.hasAttribute('data-ship')) {
                draft.selected = parseInt(target.getAttribute('data-ship'), 10) || 0;
                refresh();
                return;
            }
            var action = target.getAttribute('data-action');
            if (action === 'rotate') { draft.vertical = !draft.vertical; refresh(); return; }
            if (action === 'clear') {
                draft.fleet = [null, null, null, null, null];
                draft.selected = 0;
                refresh();
                return;
            }
            if (action === 'random') {
                var random = _randomFleet();
                if (random) { draft.fleet = random; draft.selected = 0; }
                refresh();
                return;
            }
            if (action === 'ready') {
                if (!_fleetComplete(draft.fleet) || !context.sendMove) return;
                var fleet = draft.fleet.slice();
                target.disabled = true;
                context.sendMove({ k: 'c', p: fleet }, {
                    fields: ['my_commit', 'my_board'],
                    apply: function(live) {
                        live.my_commit = 'pending';
                        live.my_board = _boardString(fleet);
                    },
                });
                return;
            }
            if (target.hasAttribute('data-cell')) {
                var cell = parseInt(target.getAttribute('data-cell'), 10);
                var occupant = _occupancy(draft.fleet, -1)[cell];
                if (occupant !== -1) {
                    // Pick the ship up so it can be placed again.
                    draft.fleet[occupant] = null;
                    draft.selected = occupant;
                    refresh();
                    return;
                }
                var next = _tryPlace(draft.fleet, draft.selected, cell, draft.vertical);
                if (!next) {
                    target.classList.add('is-invalid');
                    return;
                }
                draft.fleet = next;
                draft.selected = nextUnplaced(draft.selected + 1);
                refresh();
            }
        });
        root.addEventListener('pointerover', function(event) {
            var target = event.target.closest ? event.target.closest('.bs-place') : null;
            if (target) previewAt(parseInt(target.getAttribute('data-cell'), 10));
        });
        root.addEventListener('pointerleave', clearPreview);
        root.addEventListener('focusin', function(event) {
            var target = event.target.closest ? event.target.closest('.bs-place') : null;
            if (target) previewAt(parseInt(target.getAttribute('data-cell'), 10));
        });
    }

    function _applyOptimisticFire(live, cell) {
        var shots = _grid(live, 'shots_out', '_pmhp');
        if (!shots || shots.charAt(cell) !== '_') return;
        live.shots_out = shots.substr(0, cell) + 'p' + shots.substr(cell + 1);
        live.last_out = cell;
        live.turn = live.contact_hash;
        live.move_count = (parseInt(_meta(live, 'move_count', 0), 10) || 0) + 1;
    }

    function _bindFire(session, context) {
        if (!context || !context.root || !context.sendMove || !_isMyTurn(session, context)) return;
        var cells = context.root.querySelectorAll('.bs-fire');
        for (var i = 0; i < cells.length; i++) {
            cells[i].addEventListener('click', (function(button) {
                return function() {
                    var cell = parseInt(button.getAttribute('data-cell'), 10);
                    context.sendMove({ k: 'f', c: cell }, {
                        fields: ['shots_out', 'last_out'],
                        apply: function(live) { _applyOptimisticFire(live, cell); },
                    });
                };
            })(cells[i]));
            cells[i].addEventListener('keydown', (function(button) {
                return function(event) {
                    var cell = parseInt(button.getAttribute('data-cell'), 10);
                    var row = Math.floor(cell / GRID);
                    var column = cell % GRID;
                    if (event.key === 'ArrowLeft') column = Math.max(0, column - 1);
                    else if (event.key === 'ArrowRight') column = Math.min(GRID - 1, column + 1);
                    else if (event.key === 'ArrowUp') row = Math.max(0, row - 1);
                    else if (event.key === 'ArrowDown') row = Math.min(GRID - 1, row + 1);
                    else return;
                    event.preventDefault();
                    var next = context.root.querySelector('.bs-fire[data-cell="' + (row * GRID + column) + '"]');
                    if (next) next.focus();
                };
            })(cells[i]));
        }
    }

    function _maybeReveal(session, context) {
        if (!session || session.status !== 'active' || !context || !context.sendMove) return;
        var owes = _meta(session, 'owes_reveal', false) === true && _phase(session) === 'reveal';
        if (!owes) {
            delete _revealSentAt[session.game_id];
            return;
        }
        var last = _revealSentAt[session.game_id];
        if (last && Date.now() - last < REVEAL_RETRY_MS) return;
        _revealSentAt[session.game_id] = Date.now();
        // The reveal is mandatory for the game to finish; it proves the
        // fleet was fixed before the first shot. No user action is needed.
        context.sendMove({ k: 'r' });
    }

    function _bindBoard(session, context) {
        if (!context || !context.root) return;
        if (session.status === 'active' && _phase(session) === 'setup' && _commitState(session) === '') {
            _bindPlacement(session, context);
        }
        _bindFire(session, context);
        _maybeReveal(session, context);
    }

    function _activeStatusText(session, context) {
        var phase = _phase(session);
        var name = context && context.contactName ? context.contactName(session.contact_hash) : 'Opponent';
        if (phase === 'setup') {
            return _commitState(session) === '' ? 'Place your fleet' : 'Waiting for ' + name + '\u2019s fleet';
        }
        if (phase === 'reveal') {
            return _meta(session, 'owes_reveal', false) === true
                ? 'Revealing your fleet…'
                : 'Verifying the fleet…';
        }
        if (_isMyTurn(session, context)) return 'Your turn \u00b7 fire!';
        if (_meta(session, 'turn', '')) return name + '\u2019s turn';
        return '';
    }

    function _detailChips(session) {
        var chips = [];
        if (session.status === 'pending') return chips;
        var phase = _phase(session);
        if (phase === 'battle' || phase === 'reveal' || phase === 'done') {
            chips.push('Hits ' + _hitsOut(session) + '/' + SHIP_CELLS);
            chips.push('Sunk ' + _list(session, 'their_sunk').length + '/' + SHIPS.length);
        }
        return chips;
    }

    function _onSessionDelta(record) {
        if (!record || !record.game_id) return;
        var committed = _commitState(record);
        if (committed && committed !== 'pending') delete _drafts[record.game_id];
        if (record.status !== 'active') {
            delete _revealSentAt[record.game_id];
            if (record.status !== 'pending') delete _drafts[record.game_id];
        }
    }

    function _onSessionDeltaWithReveal(record, previous, context) {
        _onSessionDelta(record);
        _maybeReveal(record, context);
    }

    function _celebrationOptions() {
        var styles = global.getComputedStyle
            ? global.getComputedStyle(global.document.documentElement)
            : null;
        function color(name, fallback) {
            return styles ? (styles.getPropertyValue(name) || fallback).trim() : fallback;
        }
        return {
            count: 56,
            duration: 1700,
            colors: [
                color('--accent', '#D2693B'),
                color('--ble-accent-fg', '#3A839B'),
                color('--status-online-fg', '#2E8B57'),
                color('--text-secondary', '#A59D99'),
            ],
        };
    }

    if (!RS.games.views.has(APP_ID)) {
        RS.games.views.register(APP_ID, {
            displayName: 'Battleship',
            icon: '\u2693',
            themeClass: 'games-theme-ru_battleship',
            boardSelector: '.bs-wrap',
            actions: ['challenge', 'accept', 'decline', 'move', 'resign'],
            renderBoard: _renderBoard,
            bindBoard: _bindBoard,
            activeStatusText: _activeStatusText,
            detailChips: _detailChips,
            onSessionDelta: _onSessionDeltaWithReveal,
            celebrationOptions: _celebrationOptions,
        });
    }
})(window);
