// Ratspeak-ru: lxma:// address links.
//   lxma://<32 hex destination hash>
//   lxma://<32 hex destination hash>.<128 hex public key>
// Native taps and scans arrive as a typed {kind: 'lxma'} target through the existing
// native connection-link bridge; this file presents it and also handles links tapped or
// pasted inside the app. A link only pre-fills "Add contact": the user always confirms.
(function() {
    'use strict';

    var HASH_LEN = 32;
    var KEY_LEN = 128;

    function hex(part, len) {
        return typeof part === 'string' && part.length === len && /^[0-9a-fA-F]+$/.test(part)
            ? part.toLowerCase() : null;
    }

    // Mirrors parse_lxma in connection_link.rs. Returns {hash, publicKey|null} or null.
    function parse(text) {
        if (typeof text !== 'string' || text !== text.trim() || text.length > 512) return null;
        if (text.slice(0, 7).toLowerCase() !== 'lxma://') return null;
        var rest = text.slice(7);
        if (rest.charAt(rest.length - 1) === '/') rest = rest.slice(0, -1);
        var dot = rest.indexOf('.');
        var hash = hex(dot === -1 ? rest : rest.slice(0, dot), HASH_LEN);
        if (!hash) return null;
        if (dot === -1) return { hash: hash, publicKey: null };
        var key = hex(rest.slice(dot + 1), KEY_LEN);
        return key ? { hash: hash, publicKey: key } : null;
    }

    var presenting = false;

    function present(link) {
        if (!link || presenting) return true;
        var rs = window.RS;
        if (typeof rsPromptContact !== 'function' || !rs || typeof rs.invokeOrToast !== 'function') {
            return false;
        }
        presenting = true;
        rsPromptContact({ title: 'Add Contact', defaultHash: link.hash }).then(function(result) {
            presenting = false;
            if (!result) return;
            rs.invokeOrToast('add_contact', {
                args: { hash: result.hash, display_name: result.display_name }
            }, 'Could not add contact');
        }, function() { presenting = false; });
        return true;
    }

    var upstreamOpen = window.openNativeConnectionLink;
    window.openNativeConnectionLink = function(target) {
        if (target && typeof target === 'object' && target.kind === 'lxma') {
            return present({ hash: String(target.hash || ''), publicKey: target.public_key || null });
        }
        return typeof upstreamOpen === 'function' ? upstreamOpen(target) : false;
    };

    // A link tapped inside the app (message text, notes, About pages).
    document.addEventListener('click', function(event) {
        var node = event.target;
        var anchor = node && node.closest ? node.closest('a[href]') : null;
        if (!anchor) return;
        var link = parse((anchor.getAttribute('href') || '').trim());
        if (!link) return;
        event.preventDefault();
        event.stopPropagation();
        present(link);
    }, true);

    // Pasting a link into any dialog text field leaves just the hash behind.
    document.addEventListener('paste', function(event) {
        var field = event.target;
        if (!field || field.tagName !== 'INPUT' || !event.clipboardData) return;
        var link = parse((event.clipboardData.getData('text') || '').trim());
        if (!link) return;
        event.preventDefault();
        field.value = link.hash;
        field.dispatchEvent(new Event('input', { bubbles: true }));
    }, true);

    window.RuLxma = { parse: parse };
})();
