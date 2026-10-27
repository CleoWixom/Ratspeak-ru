// Route to a contact: shows the path Reticulum currently knows (hops, interface,
// next hop, time left), and can ask the network for it again.
//
// Uses the existing backend commands only:
//   api_path_query { dest_hash }  -> { has_path, hops, interface, via, expires }
//   request_path   { hash }       -> sends a path request to the transport
(function() {
    'use strict';

    var POLL_MS = 1000;
    var FIND_TIMEOUT_MS = 15000;
    var REFRESH_WAIT_MS = 6000;
    var RESEND_AFTER_MS = 5000;

    // ---- Pure helpers (unit-tested) ----------------------------------------

    // `expires` may be an absolute epoch time or the seconds left.
    function secondsLeft(expires, nowSeconds) {
        var value = Number(expires);
        if (!isFinite(value) || value <= 0) return null;
        return Math.max(0, value > 1e9 ? value - nowSeconds : value);
    }

    function formatDuration(seconds) {
        if (seconds === null || seconds === undefined) return '';
        var s = Math.round(seconds);
        if (s < 60) return s + 's';
        if (s < 3600) { var mm = Math.floor(s / 60), ss = s % 60; return mm + 'm' + (ss ? ' ' + ss + 's' : ''); }
        var h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60);
        return h + 'h' + (m ? ' ' + m + 'm' : '');
    }

    // Reduce a backend reply to what the sheet shows.
    function normalize(reply, nowSeconds) {
        if (!reply || reply.has_path !== true) {
            return { known: false, error: reply && typeof reply.error === 'string' ? reply.error : '' };
        }
        var hops = Number(reply.hops);
        return {
            known: true,
            hops: isFinite(hops) && hops >= 0 ? hops : null,
            iface: typeof reply.interface === 'string' ? reply.interface : '',
            via: typeof reply.via === 'string' && reply.via ? reply.via : '',
            left: secondsLeft(reply.expires, nowSeconds)
        };
    }

    function validHash(hash) { return typeof hash === 'string' && /^[0-9a-fA-F]{32}$/.test(hash); }

    // Ask for the path and watch until it shows up (or, when one is already
    // known, until it has had time to be refreshed). Returns the final state.
    function findRoute(hash, deps) {
        var invoke = deps.invoke, sleep = deps.sleep, now = deps.now, onState = deps.onState || function() {};
        var query = function() {
            return invoke('api_path_query', { dest_hash: hash }).then(function(reply) {
                return normalize(reply, now() / 1000);
            }, function() { return { known: false, error: '' }; });
        };
        var started = now();
        var resent = false;
        return query().then(function(first) {
            var hadPath = first.known;
            onState({ phase: 'searching', route: first });
            return invoke('request_path', { hash: hash }).then(function(reply) {
                if (reply && reply.success === false) {
                    return { phase: 'unavailable', route: first };
                }
                function poll() {
                    return sleep(POLL_MS).then(query).then(function(route) {
                        var elapsed = now() - started;
                        onState({ phase: 'searching', route: route });
                        if (!hadPath && route.known) return { phase: 'found', route: route };
                        if (hadPath && elapsed >= REFRESH_WAIT_MS) return { phase: route.known ? 'found' : 'lost', route: route };
                        if (elapsed >= FIND_TIMEOUT_MS) return { phase: route.known ? 'found' : 'none', route: route };
                        if (!resent && !route.known && elapsed >= RESEND_AFTER_MS) {
                            resent = true;
                            return invoke('request_path', { hash: hash }).then(poll, poll);
                        }
                        return poll();
                    });
                }
                return poll();
            });
        });
    }

    // ---- UI ----------------------------------------------------------------

    function short(hash) { return hash.slice(0, 8) + '…' + hash.slice(-4); }

    function rows(route) {
        if (!route.known) return [['Status', 'No path known']];
        var out = [['Status', 'Path known']];
        if (route.hops !== null) out.push(['Hops', String(route.hops)]);
        if (route.iface) out.push(['Interface', route.iface]);
        out.push(['Next hop', route.via ? short(route.via) : 'Direct']);
        if (route.left !== null) out.push(['Expires in', formatDuration(route.left)]);
        return out;
    }

    function openRouteSheet(hash, displayName) {
        if (!validHash(hash)) {
            showToast('Invalid contact address', 'toast-error', 2500);
            return;
        }
        hash = hash.toLowerCase();
        var shell = RS.sheetShell.create({ overlayClass: 'contact-card-overlay', sheetClass: 'contact-share-sheet' });
        var closed = false, busy = false;
        function close() {
            if (closed) return;
            closed = true;
            RS.sheetShell.dismiss(shell);
        }
        shell.sheet.innerHTML =
            '<div class="contact-card-topbar">' +
                '<button class="contact-card-close" type="button" aria-label="Close">&times;</button>' +
            '</div>' +
            '<div class="contact-share-identity">' +
                '<div class="contact-share-name">' + escapeHtml(displayName || short(hash)) + '</div>' +
                '<div class="contact-share-subtitle">Route to contact</div>' +
            '</div>' +
            '<div class="contact-share-address-block"><div class="route-lookup-rows" id="route-lookup-rows"></div></div>' +
            '<div class="route-lookup-note inline-hint" id="route-lookup-note"></div>' +
            '<div class="contact-share-actions">' +
                '<button class="nr-btn contact-share-action" id="route-lookup-refresh" type="button"><span>Refresh</span></button>' +
                '<button class="nr-btn contact-share-action" id="route-lookup-find" type="button"><span>Find route</span></button>' +
            '</div>';
        RS.sheetShell.present(shell);
        shell.sheet.querySelector('.contact-card-close').addEventListener('click', close);
        shell.overlay.addEventListener('click', function(e) { if (e.target === shell.overlay) close(); });
        shell.sheet._ratspeakDismiss = close;

        var rowsEl = shell.sheet.querySelector('#route-lookup-rows');
        var noteEl = shell.sheet.querySelector('#route-lookup-note');
        var findBtn = shell.sheet.querySelector('#route-lookup-find');
        var refreshBtn = shell.sheet.querySelector('#route-lookup-refresh');

        function render(route, note) {
            if (closed) return;
            rowsEl.innerHTML = rows(route).map(function(row) {
                return '<div class="route-lookup-row"><span class="route-lookup-label">' + escapeHtml(row[0]) +
                    '</span><span class="route-lookup-value">' + escapeHtml(row[1]) + '</span></div>';
            }).join('');
            noteEl.textContent = note || '';
        }
        function setBusy(value) {
            busy = value;
            findBtn.disabled = value;
            refreshBtn.disabled = value;
        }
        function current(note) {
            return RS.invoke('api_path_query', { dest_hash: hash }).then(function(reply) {
                render(normalize(reply, Date.now() / 1000), note);
            }, function() { render({ known: false }, 'Could not read the route'); });
        }
        refreshBtn.addEventListener('click', function() {
            if (busy) return;
            setBusy(true);
            current('').then(function() { setBusy(false); });
        });
        findBtn.addEventListener('click', function() {
            if (busy) return;
            setBusy(true);
            render({ known: false }, 'Searching for a route…');
            findRoute(hash, {
                invoke: RS.invoke,
                sleep: function(ms) { return new Promise(function(resolve) { setTimeout(resolve, ms); }); },
                now: Date.now,
                onState: function(state) {
                    if (state.route.known) render(state.route, 'Searching for a route…');
                }
            }).then(function(result) {
                setBusy(false);
                if (closed) return;
                if (result.phase === 'found') {
                    render(result.route, 'Route found');
                    showToast('Route found', 'toast-success', 2500);
                } else if (result.phase === 'unavailable') {
                    render(result.route, 'The network is not available');
                } else {
                    render(result.route, 'No route found. The contact may be offline or out of reach.');
                }
            }).catch(function(err) {
                setBusy(false);
                render({ known: false }, (err && err.message) || 'Could not request the route');
            });
        });
        current('');
    }

    window.openRouteSheet = openRouteSheet;
    window.RS = window.RS || {};
    RS.routeLookup = { normalize: normalize, secondsLeft: secondsLeft, formatDuration: formatDuration,
        findRoute: findRoute, validHash: validHash };
})();
