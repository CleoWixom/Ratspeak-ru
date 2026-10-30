#!/usr/bin/env node
// The app never pinch- or double-tap-zooms; only the expanded image viewer may,
// and closing it restores the lock (which also snaps zoom back to 1).
'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const root = path.resolve(__dirname, '..', '..');
const html = fs.readFileSync(path.join(root, 'dashboard/index.html'), 'utf8');
const reset = fs.readFileSync(path.join(root, 'dashboard/static/css/01-reset.css'), 'utf8');
const lxmf = fs.readFileSync(path.join(root, 'dashboard/static/js/lxmf.js'), 'utf8');

const viewport = /<meta name="viewport" content="([^"]+)">/.exec(html);
assert(viewport, 'viewport meta must exist');
assert.match(viewport[1], /maximum-scale=1\.0, user-scalable=no/);
assert.match(reset, /html:not\(\.page-zoom-allowed\),\s*html:not\(\.page-zoom-allowed\) body \{\s*touch-action: pan-x pan-y;/);

const start = lxmf.indexOf('function _setPageZoomAllowed(');
const fn = lxmf.slice(start, lxmf.indexOf('\nfunction ', start + 1));
const classes = new Set();
const meta = { content: viewport[1], getAttribute() { return this.content; }, setAttribute(_, v) { this.content = v; } };
const context = {
    document: {
        documentElement: { classList: { toggle: (c, on) => (on ? classes.add(c) : classes.delete(c)) } },
        querySelector: () => meta,
    },
};
vm.createContext(context);
vm.runInContext(fn, context);
context._setPageZoomAllowed(true);
assert(classes.has('page-zoom-allowed'));
assert.match(meta.content, /maximum-scale=5\.0, user-scalable=yes/);
assert.match(meta.content, /viewport-fit=cover/, 'other viewport settings survive');
context._setPageZoomAllowed(false);
assert(!classes.has('page-zoom-allowed'));
assert.equal(meta.content, viewport[1]);

assert(/_setPageZoomAllowed\(true\);\n\}/.test(lxmf.slice(lxmf.indexOf('function openImageViewer('))), 'the viewer unlocks zoom');
assert(lxmf.slice(lxmf.indexOf('function closeImageViewer(')).includes('_setPageZoomAllowed(false);'), 'closing relocks zoom');
console.log('page zoom lock ok');
