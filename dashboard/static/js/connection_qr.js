// Share a saved Internet/TCP or Radio connection as a QR code carrying a
// ratspeak://tcp or ratspeak://radio link (see docs/connection-links.md).
// The link is only built here; Rust (validate_connection_link) decides whether
// it is acceptable, so the QR code can never carry a link the app would reject.
(function() {
    'use strict';

    // The shared QR layout (version 13, level Q) holds 241 bytes of text.
    var QR_MAX_BYTES = 241;
    var RADIO_MODES = ['full', 'gateway', 'access_point', 'boundary', 'roaming'];

    function text(iface, keys) {
        for (var i = 0; i < keys.length; i++) {
            var value = iface && iface[keys[i]];
            if (value !== undefined && value !== null && String(value) !== '') return String(value);
        }
        return '';
    }

    function integer(iface, key) {
        var raw = text(iface, [key]);
        return /^[0-9]{1,12}$/.test(raw) ? parseInt(raw, 10) : null;
    }

    function percent(iface, key) {
        var raw = text(iface, [key]);
        if (!/^[0-9]+(\.[0-9]+)?$/.test(raw)) return null;
        var value = parseFloat(raw);
        return value > 0 && value <= 100 ? raw : null;
    }

    function byteLength(value) {
        return typeof TextEncoder === 'function'
            ? new TextEncoder().encode(value).length
            : unescape(encodeURIComponent(value)).length;
    }

    function build(kind, pairs) {
        return 'ratspeak://' + kind + '?v=1' + pairs.map(function(pair) {
            return '&' + pair[0] + '=' + encodeURIComponent(pair[1]);
        }).join('');
    }

    // Candidate links, most complete first. The optional name is dropped as a
    // fallback when it is rejected or the code would be too dense to scan.
    function candidatesFromInterface(ifaceType, iface) {
        iface = iface || {};
        var name = text(iface, ['name']);
        if (ifaceType === 'tcp_client' || ifaceType === 'backbone_client') {
            var host = text(iface, ['target_host']);
            var port = integer(iface, 'target_port');
            if (!host || port === null) return { error: 'This connection has no host or port to share' };
            var ifacName = text(iface, ['network_name', 'networkname']);
            var ifacPass = text(iface, ['passphrase', 'pass_phrase']);
            var base = [['host', host], ['port', String(port)]];
            var ifac = [];
            if (ifacName) ifac.push(['ifac_name', ifacName]);
            if (ifacPass) ifac.push(['ifac_pass', ifacPass]);
            var full = base.concat(name ? [['name', name]] : [], ifac);
            var bare = base.concat(ifac);
            return {
                kind: 'tcp',
                hasSecret: !!ifacPass,
                hasIfac: ifac.length > 0,
                summary: host + ':' + port,
                links: name ? [build('tcp', full), build('tcp', bare)] : [build('tcp', bare)]
            };
        }
        if (ifaceType === 'rnode') {
            var freq = integer(iface, 'frequency');
            var bw = integer(iface, 'bandwidth');
            var sf = integer(iface, 'spreadingfactor');
            var cr = integer(iface, 'codingrate');
            var tx = integer(iface, 'txpower');
            if ([freq, bw, sf, cr, tx].some(function(value) { return value === null; })) {
                return { error: 'This radio has no complete radio settings to share' };
            }
            var radio = [['freq', String(freq)], ['bw', String(bw)], ['sf', String(sf)],
                ['cr', String(cr)], ['tx', String(tx)]];
            var mode = text(iface, ['mode', 'interface_mode']).toLowerCase();
            if (RADIO_MODES.indexOf(mode) !== -1) radio.push(['mode', mode]);
            var airtimeShort = percent(iface, 'airtime_limit_short');
            var airtimeLong = percent(iface, 'airtime_limit_long');
            if (airtimeShort) radio.push(['airtime_short', airtimeShort]);
            if (airtimeLong) radio.push(['airtime_long', airtimeLong]);
            var named = (name ? [['name', name]] : []).concat(radio);
            return {
                kind: 'radio',
                hasSecret: false,
                hasIfac: false,
                summary: (freq / 1e6).toFixed(3).replace(/0+$/, '').replace(/\.$/, '') +
                    ' MHz · ' + (bw / 1000) + ' kHz · SF' + sf + ' · CR' + cr,
                links: name ? [build('radio', named), build('radio', radio)] : [build('radio', radio)]
            };
        }
        return { error: 'This connection cannot be shared as a QR code' };
    }

    // Pick the first candidate Rust accepts that also fits the QR layout.
    function resolve(ifaceType, iface, validate) {
        var plan = candidatesFromInterface(ifaceType, iface);
        if (plan.error) return Promise.reject(new Error(plan.error));
        var lastError = null;
        var index = 0;
        function next() {
            if (index >= plan.links.length) {
                return Promise.reject(lastError || new Error('Could not create a QR code for this connection'));
            }
            var link = plan.links[index++];
            if (byteLength(link) > QR_MAX_BYTES) {
                lastError = new Error('This connection is too long for a QR code');
                return next();
            }
            return Promise.resolve(validate(link)).then(function() {
                return { link: link, kind: plan.kind, summary: plan.summary,
                    hasSecret: plan.hasSecret, hasIfac: plan.hasIfac };
            }, function(err) {
                lastError = err instanceof Error ? err : new Error(String(err || 'The link was rejected'));
                return next();
            });
        }
        return next();
    }

    function fileBase(name) {
        var base = String(name || 'connection').replace(/[^A-Za-z0-9._-]+/g, '-').replace(/^-+|-+$/g, '');
        return (base || 'connection').slice(0, 40);
    }

    function openConnectionQrShare(ifaceType, ifaceName) {
        var record = (typeof _cachedConfigByName !== 'undefined' && _cachedConfigByName &&
            _cachedConfigByName[ifaceName]) || null;
        var iface = record ? record.iface : null;
        if (!iface) {
            showToast('Interface settings are still loading', 'toast-warning', 2500);
            return;
        }
        resolve(ifaceType, iface, function(link) {
            return RS.invoke('validate_connection_link', { payload: link });
        }).then(function(result) {
            present(ifaceName, result);
        }).catch(function(err) {
            showToast((err && err.message) || 'Could not create a QR code for this connection', 'toast-error', 4000);
        });
    }

    function present(ifaceName, result) {
        var shell = RS.sheetShell.create({
            overlayClass: 'contact-card-overlay',
            sheetClass: 'contact-share-sheet'
        });
        var closed = false;
        function close() {
            if (closed) return;
            closed = true;
            RS.sheetShell.dismiss(shell);
        }
        var isRadio = result.kind === 'radio';
        var warning = result.hasSecret
            ? '<div class="inline-warning">This QR code contains the IFAC passphrase as plain text. Share it only with people you trust.</div>'
            : '';
        var hint = isRadio
            ? '<div class="inline-hint">The radio device is not included: the other person picks their own.</div>'
            : '';
        shell.sheet.innerHTML =
            '<div class="contact-card-topbar">' +
                '<button class="contact-card-close" type="button" aria-label="Close">&times;</button>' +
            '</div>' +
            '<div class="contact-share-identity">' +
                '<div class="contact-share-name">' + escapeHtml(ifaceName) + '</div>' +
                '<div class="contact-share-subtitle">' + (isRadio ? 'Radio connection' : 'Internet/TCP connection') + '</div>' +
            '</div>' +
            '<div class="contact-share-qr-shell">' +
                '<canvas class="contact-share-qr" aria-label="Connection QR code"></canvas>' +
            '</div>' +
            '<div class="contact-share-address-block">' +
                '<div class="contact-share-address mono">' + escapeHtml(result.summary) + '</div>' +
            '</div>' +
            warning + hint +
            '<div class="contact-share-actions">' +
                '<button class="nr-btn contact-share-action" id="connection-qr-copy" type="button"><span>Copy link</span></button>' +
                '<button class="nr-btn contact-share-action" id="connection-qr-share" type="button" disabled><span>Share QR</span></button>' +
            '</div>';
        RS.sheetShell.present(shell);
        shell.sheet.querySelector('.contact-card-close').addEventListener('click', close);
        shell.overlay.addEventListener('click', function(e) { if (e.target === shell.overlay) close(); });
        shell.sheet._ratspeakDismiss = close;

        var canvas = shell.sheet.querySelector('canvas');
        var shareButton = shell.sheet.querySelector('#connection-qr-share');
        shell.sheet.querySelector('#connection-qr-copy').addEventListener('click', function() {
            RS.copyText(result.link).then(function(ok) {
                showToast(ok ? 'Link copied' : 'Could not copy the link', ok ? 'toast-success' : 'toast-error', 2500);
            });
        });
        shareButton.addEventListener('click', function() {
            RS.qr.shareCanvas(canvas, fileBase(ifaceName) + '-ratspeak-connection.png', 'Ratspeak connection')
                .then(function(method) {
                    showToast(method === 'share' ? 'QR code shared' : 'QR image saved', 'toast-success', 2500);
                }).catch(function(err) {
                    if (err && err.name === 'AbortError') return;
                    showToast((err && err.message) || 'Could not share QR', 'toast-error', 3000);
                });
        });
        // Let the sheet paint before the synchronous QR encoder runs.
        var frame = window.requestAnimationFrame || function(callback) { return setTimeout(callback, 0); };
        frame(function() {
            frame(function() {
                if (closed || !canvas.isConnected) return;
                try {
                    RS.qr.renderCanvas(canvas, result.link);
                    shareButton.disabled = false;
                } catch (err) {
                    showToast((err && err.message) || 'Could not render QR', 'toast-error', 3000);
                }
            });
        });
    }

    window.openConnectionQrShare = openConnectionQrShare;
    window.RS = window.RS || {};
    RS.connectionQr = { candidatesFromInterface: candidatesFromInterface, resolve: resolve, maxBytes: QR_MAX_BYTES };
})();
