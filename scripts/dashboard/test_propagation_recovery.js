'use strict';
const assert = require('assert');
const fs = require('fs');
const path = require('path');
const vm = require('vm');
const source = fs.readFileSync(path.join(__dirname, '../../dashboard/static/js/propagation.js'), 'utf8');
const calls = [], toasts = [], handlers = {};
let confirm = false;
const ctx = {
    window: {},
    document: { getElementById: id => id === 'prop-retry-deferred-btn' ? {
        addEventListener: (event, fn) => { handlers.retry = fn; }
    } : null },
    escapeHtml: value => String(value).replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c])),
    activeIdentity: () => ({ is_hardware: false }),
    rsConfirm: async () => confirm,
    rsPrompt: async args => args.placeholder.includes('64') ? 'a'.repeat(64) : '/tmp/approved.json',
    showToast: (...args) => toasts.push(args),
    RS: { invoke: async (command, args) => { calls.push({command,args}); return {quarantined_files:2,quarantined_bytes:24}; } },
    setTimeout: () => 0,
};
vm.createContext(ctx);
vm.runInContext(source.slice(0, source.indexOf("RS.listen(")), ctx);
(async () => {
    ctx.propagationStatus = {};
    assert.equal(ctx.renderDeferredDownloads(), '');
    ctx.propagationStatus = {suppressed_count:2,deferred_count:1,admission_paused:true,suppression_full:true,next_retry:1800000000};
    const deferred = ctx.renderDeferredDownloads();
    assert(deferred.includes('2 messages could not be accepted'));
    assert(deferred.includes('1 message will be retried'));
    assert(deferred.includes('retry list is full'));
    assert(deferred.includes('Next retry:'));
    ctx.propagationStatus = {hosting_requested:true,hosting_enabled:false,hosting_status:{identity:'<profile>',readiness:'migration_required',reason:'<unverified>',quarantined_files:2,quarantined_bytes:24}};
    const blocked = ctx.renderHostingSettings();
    assert(blocked.includes('id="prop-host-toggle" checked'));
    assert(blocked.includes('Hosting unavailable'));
    assert(blocked.includes('&lt;unverified&gt;') && !blocked.includes('<profile>'));
    assert(blocked.includes('2 archived files preserved (24 bytes)'));
    assert(!blocked.includes('id="prop-host-announce-btn"'));
    await ctx.recoverHostedInbox(false);
    assert.equal(calls.length,0,'declining writer exclusion must not start migration');
    confirm = true;
    await ctx.recoverHostedInbox(true);
    const recovery = calls.find(c => c.command === 'recover_propagation_host');
    assert(recovery);
    assert.equal(recovery.args.args.identity,'<profile>');
    assert.equal(recovery.args.args.manifest_path,'/tmp/approved.json');
    assert.equal(recovery.args.args.approved_manifest_sha256,'a'.repeat(64));
    assert.equal(recovery.args.args.supervisor_writers_stopped,true);
    ctx.wireUpHandlers({querySelectorAll:()=>[]},'manual');
    await handlers.retry();
    assert(calls.some(c => c.command === 'retry_deferred_propagation'));
    assert(calls.some(c => c.command === 'sync_propagation'));
    ctx.activeIdentity = () => ({is_hardware:true});
    assert(!ctx.renderHostingSettings().includes('id="prop-host-toggle"'));
    console.log('propagation recovery: 20 assertions passed');
})().catch(error => {console.error(error);process.exitCode=1;});
