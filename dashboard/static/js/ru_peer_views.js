// Ratspeak-ru: display modes for the home "Recent Peers" panel.
// Wraps upstream renderDashboardPeersList (health.js) and post-processes the rows it
// renders, so no upstream file is edited. Modes: default, grouped by status, grouped by
// contacts, compact list, tiles. The choice is stored in localStorage.
(function() {
    'use strict';

    var KEY = 'ratspeak-ru-peers-view';
    var MODES = ['default', 'status', 'contacts', 'list', 'tiles'];
    var LABELS = {
        en: { default: 'Default', status: 'Group by status', contacts: 'Group by contacts', list: 'Compact list', tiles: 'Tiles', view: 'Peer view',
              reachable: 'Reachable', stale: 'Stale', offline: 'Offline', contact: 'Contacts', other: 'Other peers' },
        ru: { default: 'По умолчанию', status: 'По статусу', contacts: 'По контактам', list: 'Список', tiles: 'Плитки', view: 'Вид пиров',
              reachable: 'Доступны', stale: 'Давно не видны', offline: 'Не в сети', contact: 'Контакты', other: 'Остальные пиры' }
    };

    function lang() {
        var l = 'en';
        try { l = localStorage.getItem('ratspeak_lang') || (navigator.language || 'en').slice(0, 2); } catch (_) {}
        return l === 'ru' ? 'ru' : 'en';
    }
    function t(key) { return LABELS[lang()][key] || LABELS.en[key] || key; }

    function readMode() {
        try {
            var v = localStorage.getItem(KEY);
            return MODES.indexOf(v) !== -1 ? v : 'default';
        } catch (_) { return 'default'; }
    }
    function writeMode(mode) {
        try {
            if (mode === 'default') localStorage.removeItem(KEY);
            else localStorage.setItem(KEY, mode);
        } catch (_) {}
    }

    function statusBucket(status) {
        if (status === 'reachable' || status === 'direct') return 'reachable';
        if (status === 'stale') return 'stale';
        return 'offline';
    }

    // Pure: split items ({hash, status, is_contact}) into ordered, non-empty groups.
    function planGroups(items, mode) {
        var order = mode === 'contacts' ? ['contact', 'other'] : ['reachable', 'stale', 'offline'];
        var groups = order.map(function(key) { return { key: key, hashes: [] }; });
        items.forEach(function(item) {
            var key = mode === 'contacts' ? (item.is_contact ? 'contact' : 'other') : statusBucket(item.status);
            groups[order.indexOf(key)].hashes.push(item.hash);
        });
        return groups.filter(function(g) { return g.hashes.length > 0; });
    }

    function rowStatus(row) {
        var dot = row.querySelector('.dashboard-peers-dot');
        var m = dot && /status-([a-z]+)/.exec(dot.className);
        return m ? m[1] : 'unknown';
    }

    function contactSet() {
        var set = {};
        try {
            ((typeof PeersCache !== 'undefined' && PeersCache && PeersCache.enriched()) || []).forEach(function(p) {
                if (p.is_contact) set[p.hash] = true;
            });
        } catch (_) {}
        return set;
    }

    var collapsed = {};

    function applyMode(listEl, mode) {
        MODES.forEach(function(m) { listEl.classList.remove('ru-peers-mode-' + m); });
        listEl.classList.add('ru-peers-mode-' + mode);
        if (mode !== 'status' && mode !== 'contacts') return;

        var direct = [];
        for (var i = 0; i < listEl.children.length; i++) {
            if (listEl.children[i].classList.contains('dashboard-peers-row')) direct.push(listEl.children[i]);
        }
        if (!direct.length) return; // already grouped (upstream skipped re-render) or empty

        var contacts = mode === 'contacts' ? contactSet() : {};
        var byHash = {};
        var items = direct.map(function(row) {
            var hash = row.getAttribute('data-hash') || '';
            byHash[hash] = row;
            return { hash: hash, status: rowStatus(row), is_contact: !!contacts[hash] };
        });
        var tail = [];
        for (var j = 0; j < listEl.children.length; j++) {
            if (!listEl.children[j].classList.contains('dashboard-peers-row')) tail.push(listEl.children[j]);
        }

        var frag = document.createDocumentFragment();
        planGroups(items, mode).forEach(function(group) {
            var id = mode + ':' + group.key;
            var section = document.createElement('div');
            section.className = 'ru-peer-group' + (collapsed[id] ? ' collapsed' : '');
            var head = document.createElement('button');
            head.type = 'button';
            head.className = 'ru-peer-group-head';
            head.textContent = t(group.key) + ' · ' + group.hashes.length;
            head.addEventListener('click', function() {
                collapsed[id] = !collapsed[id];
                section.classList.toggle('collapsed', !!collapsed[id]);
            });
            section.appendChild(head);
            var body = document.createElement('div');
            body.className = 'ru-peer-group-body';
            group.hashes.forEach(function(h) { body.appendChild(byHash[h]); });
            section.appendChild(body);
            frag.appendChild(section);
        });
        tail.forEach(function(node) { frag.appendChild(node); });
        listEl.appendChild(frag);
    }

    function refresh(force) {
        var listEl = document.getElementById('dashboard-peers-list');
        if (!listEl) return;
        if (force) window._lastPeersJson = ''; // makes upstream re-render once
        if (typeof window.renderDashboardPeersList === 'function') window.renderDashboardPeersList();
    }

    function buildSwitcher() {
        var actions = document.querySelector('#panel-dashboard-peers .panel-header-actions');
        if (!actions || document.getElementById('ru-peers-view-select')) return;
        var select = document.createElement('select');
        select.id = 'ru-peers-view-select';
        select.className = 'nr-input-sm ru-peers-view-select';
        select.setAttribute('aria-label', t('view'));
        select.title = t('view');
        MODES.forEach(function(m) {
            var opt = document.createElement('option');
            opt.value = m;
            opt.textContent = t(m);
            select.appendChild(opt);
        });
        select.value = readMode();
        select.addEventListener('change', function() {
            writeMode(select.value);
            refresh(true);
        });
        actions.insertBefore(select, actions.firstChild);
    }

    window.RuPeerViews = { MODES: MODES, planGroups: planGroups, statusBucket: statusBucket, readMode: readMode };

    var original = window.renderDashboardPeersList;
    if (typeof original !== 'function' || original.__ruWrapped) return;
    var wrapper = function() {
        var result = original.apply(this, arguments);
        var listEl = document.getElementById('dashboard-peers-list');
        if (listEl) applyMode(listEl, readMode());
        return result;
    };
    wrapper.__ruWrapped = true;
    window.renderDashboardPeersList = wrapper;

    function init() { buildSwitcher(); refresh(true); }
    if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', init);
    else init();
})();
