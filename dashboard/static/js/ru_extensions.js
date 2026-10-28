// Ratspeak-ru: menu entries and event listeners that extend upstream screens
// without editing upstream files. Everything here is added by wrapping a public
// global at load time, so a Ratspeak update that does not rename those globals or
// the menu labels below keeps working unchanged.
// (test_fork_overlay.js checks that every anchor used here still exists upstream.)
(function() {
    'use strict';

    var ICON_QR = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="7" height="7" rx="1"/><rect x="14" y="3" width="7" height="7" rx="1"/><rect x="3" y="14" width="7" height="7" rx="1"/><path d="M14 14h3v3h-3zM20 14v.01M14 20h.01M17 20h4v-3"/></svg>';
    var ICON_ROUTE = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="5" cy="18" r="2"/><circle cx="19" cy="6" r="2"/><path d="M7 18h6a4 4 0 0 0 0-8h-2a4 4 0 0 1 0-8"/><path d="M17 6h-2"/></svg>';

    function indexOfLabel(items, label) {
        for (var i = 0; i < items.length; i++) {
            if (items[i] && items[i].label === label) return i;
        }
        return -1;
    }

    function insertAt(items, index, item) {
        var copy = items.slice();
        copy.splice(index < 0 ? copy.length : index, 0, item);
        return copy;
    }

    // ---- Connection menu: Share QR code ------------------------------------

    function qrShareable(ifaceType) {
        if (ifaceType === 'tcp_client' || ifaceType === 'backbone_client') return true;
        return typeof isLoraInterfaceType === 'function' && isLoraInterfaceType(ifaceType);
    }

    function withQrItem(items, ifaceType, ifaceName) {
        if (!Array.isArray(items) || !qrShareable(ifaceType) || indexOfLabel(items, 'Share QR code') !== -1) return items;
        return insertAt(items, indexOfLabel(items, 'Rename'), {
            label: 'Share QR code',
            icon: ICON_QR,
            onSelect: function() { openConnectionQrShare(ifaceType, ifaceName); }
        });
    }

    // ---- Contact menus: Route ----------------------------------------------

    function routeItem(hash, name, beforeOpen) {
        return {
            label: 'Route',
            icon: ICON_ROUTE,
            onSelect: function() {
                if (beforeOpen) beforeOpen();
                openRouteSheet(hash, name);
            }
        };
    }

    // Returns the menu with a Route entry when it is a contact menu, else unchanged.
    function withRouteItem(items, trigger) {
        if (!Array.isArray(items) || indexOfLabel(items, 'Route') !== -1) return items;

        // Conversation header menu.
        var copyIndex = indexOfLabel(items, 'Copy LXMF Hash');
        if (copyIndex !== -1 && typeof lxmfActiveContact === 'string' && lxmfActiveContact) {
            var hash = lxmfActiveContact;
            var contact = typeof lxmfContacts !== 'undefined' && Array.isArray(lxmfContacts)
                ? lxmfContacts.filter(function(c) { return c.hash === hash; })[0] : null;
            return insertAt(items, copyIndex, routeItem(hash, contact ? contact.display_name : ''));
        }

        // Contact sheet "more" menu (Block).
        if (trigger && trigger.id === 'conn-sheet-more-btn' && trigger.dataset && trigger.dataset.hash &&
                items.length && items[0] && items[0].label === 'Block') {
            var h = trigger.dataset.hash;
            var name = '';
            if (typeof _connsView === 'function') {
                _connsView().forEach(function(c) { if (c.hash === h) name = c.display_name || ''; });
            }
            return insertAt(items, 0, routeItem(h, name, function() {
                if (typeof closeConnectionDetailSheet === 'function') closeConnectionDetailSheet();
            }));
        }
        return items;
    }

    // ---- Wiring ------------------------------------------------------------

    function wrap(name, make) {
        var original = window[name];
        if (typeof original !== 'function' || original._ratspeakRuWrapped) return;
        var wrapped = make(original);
        wrapped._ratspeakRuWrapped = true;
        window[name] = wrapped;
    }

    function install() {
        wrap('buildIfaceActionItems', function(original) {
            return function(ifaceType, ifaceName) {
                return withQrItem(original.apply(this, arguments), ifaceType, ifaceName);
            };
        });
        wrap('actionPopover', function(original) {
            return function(trigger, items) {
                var args = Array.prototype.slice.call(arguments);
                try { args[1] = withRouteItem(items, trigger); } catch (_) { /* keep the original menu */ }
                return original.apply(this, args);
            };
        });
        if (window.RS && typeof RS.listen === 'function') {
            var listen = function(name, handler) {
                try {
                    var pending = RS.listen(name, handler);
                    if (pending && typeof pending.catch === 'function') pending.catch(function() {});
                } catch (_) { /* event bridge not ready */ }
            };
            listen('stats_update', function(data) {
                if (typeof announceRadarUpdate === 'function') announceRadarUpdate((data && data.path_table) || []);
            });
            listen('announce_received', function(data) {
                if (typeof announceRadarPulse === 'function') announceRadarPulse(data);
            });
        }
    }

    window.RS = window.RS || {};
    RS.ruExtensions = { withQrItem: withQrItem, withRouteItem: withRouteItem, install: install };
    install();
})();
