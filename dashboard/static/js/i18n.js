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

    function tr(s) {
        var t = s.replace(/\s+/g, ' ').trim();
        var v = dict[t];
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
