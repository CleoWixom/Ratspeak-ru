/* Лёгкий рантайм-переводчик интерфейса.
 * Словарь: window.RATSPEAK_I18N = { "English text": "Перевод", ... } (см. static/locales/*.js).
 * Язык: localStorage['ratspeak_lang'] или язык системы; 'en' — без перевода.
 * Переводит текстовые узлы и атрибуты placeholder/title/aria-label/data-tooltip/alt,
 * следит за DOM через MutationObserver, поэтому код приложения менять не нужно.
 */
(function () {
    'use strict';
    var dict = window.RATSPEAK_I18N || {};
    var lang = 'en';
    try { lang = localStorage.getItem('ratspeak_lang') || (navigator.language || 'en').slice(0, 2); } catch (e) {}
    if (lang === 'en' || !Object.keys(dict).length) return;
    document.documentElement.lang = lang;

    var ATTRS = ['placeholder', 'title', 'aria-label', 'data-tooltip', 'alt'];
    var SKIP = { SCRIPT: 1, STYLE: 1, CODE: 1, PRE: 1, TEXTAREA: 1, SVG: 1 };

    var pats = (window.RATSPEAK_I18N_PATTERNS || []).map(function (p) { return [new RegExp(p[0]), p[1]]; });

    // Русский плюрал: форма для 1, 2–4, 5+ (11–14 — «5+»).
    function plural(n, forms) {
        var a = forms.split('|'), i = Math.abs(parseInt(n, 10)), m10 = i % 10, m100 = i % 100;
        if (isNaN(i)) return a[a.length - 1];
        if (m10 === 1 && m100 !== 11) return a[0];
        if (m10 >= 2 && m10 <= 4 && (m100 < 12 || m100 > 14)) return a[1] || a[0];
        return a[2] || a[a.length - 1];
    }
    function fill(tpl, groups) {
        return tpl.replace(/\{(\d+)(?::([^}]*))?\}/g, function (_, idx, forms) {
            var g = groups[+idx];
            if (g === undefined) return '';
            if (forms) return g.replace(/^\s+|\s+$/g, '') + ' ' + plural(g, forms);
            return dict[g.trim()] ? g.replace(g.trim(), dict[g.trim()]) : g;
        });
    }
    function tr(s) {
        var t = s.replace(/\s+/g, ' ').trim();
        var v = dict[t];
        if (!v && t.length < 800) {
            for (var i = 0; i < pats.length; i++) {
                var m = pats[i][0].exec(t);
                if (m) { v = fill(pats[i][1], m.slice(1)); break; }
            }
        }
        if (!v) return null;
        var lead = s.match(/^\s*/)[0], trail = s.match(/\s*$/)[0];
        return lead + v + trail;
    }
    function node(n) {
        if (n.nodeType === 3) {
            if (n.parentNode && SKIP[n.parentNode.nodeName.toUpperCase()]) return;
            var r = tr(n.nodeValue);
            if (r !== null && r !== n.nodeValue) n.nodeValue = r;
        } else if (n.nodeType === 1) {
            if (SKIP[n.nodeName.toUpperCase()]) return;
            for (var i = 0; i < ATTRS.length; i++) {
                var v = n.getAttribute(ATTRS[i]);
                if (v) { var r2 = tr(v); if (r2 !== null && r2 !== v) n.setAttribute(ATTRS[i], r2); }
            }
            for (var c = n.firstChild; c; c = c.nextSibling) node(c);
        }
    }
    var busy = false;
    var mo = new MutationObserver(function (list) {
        if (busy) return;
        busy = true;
        list.forEach(function (m) {
            if (m.type === 'childList') m.addedNodes.forEach(node);
            else if (m.type === 'characterData') node(m.target);
            else if (m.type === 'attributes') node(m.target);
        });
        busy = false;
    });
    function start() {
        node(document.body);
        document.title = tr(document.title) || document.title;
        mo.observe(document.body, { subtree: true, childList: true, characterData: true,
            attributes: true, attributeFilter: ATTRS });
    }
    if (document.body) start(); else document.addEventListener('DOMContentLoaded', start);
})();
