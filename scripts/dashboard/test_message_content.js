#!/usr/bin/env node
'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const source = fs.readFileSync(path.join(__dirname, '../../dashboard/static/js/lxmf.js'), 'utf8');
const context = { window: {} };
vm.createContext(context);
for (const name of ['_messageBodyContent', '_messageDisplayContent', '_messagePlainContent', '_messageIsMarkdown',
    '_stripMessageMarkdown', '_conversationPreviewForMessage']) {
    const start = source.indexOf('function ' + name + '(');
    assert(start >= 0);
    vm.runInContext(source.slice(start, source.indexOf('\nfunction ', start + 1)), context);
}
const display = context._messageDisplayContent;
assert.equal(display({title: ' A title ', content: ''}, false), 'A title');
assert.equal(display({title: '日本語', content: 'Привет'}, false), '日本語\n\nПривет');
assert.equal(display({title: ' ', content: 'Original\nbody'}, false), 'Original\nbody');
assert.equal(display({title: '<script>', content: 'body'}, false), '<script>\n\nbody');
assert.equal(display({title: 'Photo title', content: '[File: photo.png]', image: {}}, false), 'Photo title');
assert.equal(display({title: 'Memo title', content: 'Voice message'}, true), 'Memo title');
assert.equal(display({content: 'Voice message'}, true), '');
assert.equal(context._conversationPreviewForMessage({title: 'Title', content: ''}), 'Title');
assert.equal(context._conversationPreviewForMessage({audio: {mode: 0xfe}}), 'Unsupported audio');
assert(source.includes("var text = _messageDisplayContent(msg, hasRenderedAudio);\n    return { html: text ? linkifyMessageText(text) : '', markdown: false };"),
    'plain messages and titles keep the existing escaped renderer');

// FIELD_RENDERER = Markdown messages preview without syntax; others stay verbatim.
vm.runInContext(fs.readFileSync(path.join(__dirname, '../../dashboard/static/js/markdown.js'), 'utf8'), context);
context.RS = context.window.RS;
const preview = context._conversationPreviewForMessage;
assert.equal(preview({content: '**Hi** ||secret||', renderer: 2}), 'Hi [Spoiler]');
assert.equal(preview({content: '**Hi** ||secret||', renderer: null}), '**Hi** ||secret||');
assert.equal(preview({title: 'T', content: '# Head', renderer: 2}), 'T\n\nHead');
assert.equal(display({content: '# Head', renderer: 2}, false), '# Head', 'display content stays the source for Copy');
assert(source.includes('var replyContent = _messagePlainContent(msgData, !!msgData.audio)'), 'reply quotes include titles');
assert(source.includes('var messageText = _messageCopyText(msgData, content);'), 'Copy goes through spoiler-aware copy text');
assert(/function _messageCopyText\(msg, content\) \{\n    var text = _messageDisplayContent\(msg, /.test(source), 'Copy Message includes titles');
assert(source.includes('var notifBody = _messagePlainContent(msg, !!msg.audio)'), 'browser notifications include titles');
console.log('message title/body presentation: PASS');
