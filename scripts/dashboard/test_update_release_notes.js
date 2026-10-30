#!/usr/bin/env node
// Update check: release notes drop a heading that only repeats the version,
// tolerate CRLF, and the Get Update URL is built from constants plus a
// validated tag, never from the remote html_url.
'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const root = path.resolve(__dirname, '..', '..');
const source = fs.readFileSync(path.join(root, 'dashboard/static/js/settings.js'), 'utf8');
const context = {};
vm.createContext(context);
for (const marker of ['var RATSPEAK_RELEASES_URL', 'var RATSPEAK_RELEASE_TAG_RE']) {
    const start = source.indexOf(marker);
    vm.runInContext(source.slice(start, source.indexOf('\n', start)), context);
}
for (const name of ['_settingsNormalizeVersion', '_settingsReleaseNotesBody', '_settingsReleaseUrl']) {
    const start = source.indexOf('function ' + name + '(');
    assert(start >= 0, name);
    vm.runInContext(source.slice(start, source.indexOf('\nfunction ', start + 1)), context);
}
const notes = context._settingsReleaseNotesBody;
const url = context._settingsReleaseUrl;

assert.equal(notes('## v1.0.35\n\n### Added\n\n- One', '1.0.35'), '### Added\n\n- One');
assert.equal(notes('\r\n# Ratspeak v1.0.35\r\n\r\n- One\r\n', '1.0.35'), '- One', 'CRLF and a named heading');
assert.equal(notes('## Highlights\n\n- One', '1.0.35'), '## Highlights\n\n- One', 'other headings stay');
assert.equal(notes('## v1.0.34\n- One', '1.0.35'), '## v1.0.34\n- One', 'a different version stays');
assert.equal(notes('', '1.0.35'), '');
assert.equal(notes(null, '1.0.35'), '');

assert.equal(url('v1.0.36'), 'https://github.com/ratspeak/Ratspeak/releases/tag/v1.0.36');
assert.equal(url('1.2.3-rc.1'), 'https://github.com/ratspeak/Ratspeak/releases/tag/1.2.3-rc.1');
assert.equal(url('../../evil'), 'https://github.com/ratspeak/Ratspeak/releases');
assert.equal(url('v1.0.36/../../x'), 'https://github.com/ratspeak/Ratspeak/releases');
assert.equal(url(''), 'https://github.com/ratspeak/Ratspeak/releases');

assert(source.includes('RS.markdown.render(body'), 'notes render through the escaping Markdown renderer');
assert(!/html_url/.test(source), 'the remote html_url is never used');
assert(!/innerHTML = (release|body|data)/.test(source), 'raw remote text never reaches innerHTML');
console.log('update release notes ok');
