#!/usr/bin/env node
'use strict';

// A core that could not start manages no command state. The window carries the
// failure from its initialization script and shows a static screen instead of
// the app, without a single invoke.

const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const js = path.join(__dirname, '..', '..', 'dashboard', 'static', 'js');
const read = name => fs.readFileSync(path.join(js, name), 'utf8');
function range(source, start, end) {
    const from = start ? source.indexOf(start) : 0;
    const to = source.indexOf(end, from);
    assert(from >= 0 && to > from, 'missing source range ' + start + ' .. ' + end);
    return source.slice(from, to);
}
const invokeSource = range(read('state.js'), 'function _rsInvokeAvailable()', '// Copy text to the clipboard');
const bootstrapSource = range(read('tauri_events.js'), '', "if (document.readyState === 'complete'");
const setupSource = range(read('setup.js'), '', 'function enterSetupMode()');
const flush = () => new Promise(resolve => setImmediate(resolve));

class FakeElement {
    constructor(tagName) {
        this.tagName = tagName;
        this.id = '';
        this.className = '';
        this.textContent = '';
        this.style = {};
        this.attributes = {};
        this.children = [];
    }
    setAttribute(name, value) { this.attributes[name] = String(value); }
    appendChild(child) { this.children.push(child); return child; }
    text() { return [this.textContent].concat(this.children.map(child => child.text())).join(' '); }
}

function page(failure) {
    const classes = new Set(['checking-setup']);
    const body = new FakeElement('body');
    body.classList = {
        add: name => classes.add(name),
        remove: name => classes.delete(name),
        contains: name => classes.has(name),
        toggle: (name, on) => (on ? classes.add(name) : classes.delete(name))
    };
    const context = {
        Promise, setImmediate, console, setTimeout, setInterval, clearInterval,
        document: {
            body,
            readyState: 'complete',
            createElement: tagName => new FakeElement(tagName),
            createElementNS: (_namespace, tagName) => new FakeElement(tagName),
            getElementById: () => null
        },
        invoked: [],
        toasts: []
    };
    context.window = context;
    if (failure !== undefined) context.__RATSPEAK_STARTUP_ERROR__ = failure;
    context.__TAURI_INTERNALS__ = {
        invoke: (name) => {
            context.invoked.push(name);
            return Promise.resolve(name === 'api_startup_progress'
                ? { stage: 'ready', storage_warning: 'On a network drive.', profile_lock_warning: 'Cannot lock.' }
                : name === 'api_setup_status' ? { needs_setup: false } : []);
        }
    };
    context.showToast = (message, intent) => context.toasts.push([message, intent]);
    vm.runInNewContext('window.RS = window.RS || {};\n' + invokeSource, context, { filename: 'state.js' });
    vm.runInNewContext('var RS = window.RS;\n' + bootstrapSource, context, { filename: 'tauri_events.js' });
    vm.runInNewContext(setupSource, context, { filename: 'setup.js' });
    return { context, body, classes };
}

const failures = [];
async function check(name, run) {
    try {
        await run();
        console.log('ok - ' + name);
    } catch (error) {
        failures.push(name);
        console.log('not ok - ' + name + '\n' + (error && error.stack || error));
    }
}

const screens = [
    [{ code: 'profile_in_use' }, ['Ratspeak is already open', 'Quit it, then open Ratspeak again']],
    [{ code: 'schema_newer', found: 46, supported: 45 }, ['Update Ratspeak', 'newer version of Ratspeak', '46', '45']],
    [{ code: 'storage' }, ["Ratspeak can't open its data", 'Restart Ratspeak']],
    [{ code: 'from_a_later_build' }, ["Ratspeak can't open its data"]]
];

(async () => {
    for (const [failure, copy] of screens) {
        await check('a ' + failure.code + ' start shows its static screen before any invoke', async () => {
            const { context, body } = page(failure);
            context._rsBootstrapOnLoad();
            await flush();
            assert.deepEqual(context.invoked, [], 'a failed start must not invoke');
            const screen = body.children.find(child => child.id === 'startup-error-overlay');
            assert(screen, 'the static startup screen must be shown');
            assert.equal(screen.attributes.role, 'alert');
            assert.equal(screen.style.display, 'flex');
            for (const phrase of copy) assert(screen.text().includes(phrase), 'missing copy: ' + phrase);
        });
    }

    await check('every command refuses while the core has not started', async () => {
        const { context } = page({ code: 'storage' });
        await assert.rejects(context.RS.invoke('api_version'), error => error.code === 'startup_error');
        assert.deepEqual(context.invoked, []);
    });

    await check('the setup check keeps the app hidden behind the static screen', async () => {
        const { context, classes } = page({ code: 'profile_in_use' });
        context.checkSetupStatus();
        await flush();
        assert(classes.has('checking-setup'));
    });

    await check('a started core bootstraps as before', async () => {
        const { context, body, classes } = page();
        context._rsBootstrapOnLoad();
        context.checkSetupStatus();
        await flush();
        assert(context.invoked.includes('api_startup_progress'));
        assert(!body.children.some(child => child.id === 'startup-error-overlay'));
        assert(!classes.has('checking-setup'));
    });

    await check('the profile lock warning shows beside the storage warning', async () => {
        const { context } = page();
        context._rsBootstrapOnLoad();
        await flush();
        assert.deepEqual(context.toasts, [
            ['On a network drive.', 'toast-warning'],
            ['Cannot lock.', 'toast-warning']
        ]);
    });

    if (failures.length) {
        console.log(failures.length + ' failed');
        process.exit(1);
    }
})();
