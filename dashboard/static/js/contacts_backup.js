// Settings → Identity: export and import contacts plus all Internet/TCP
// connections as one JSON file.
//
// Importing never trusts the file: every field is validated here, and each item
// is then added through the same backend commands as the normal forms
// (add_contact, add_tcp_connection, add_backbone_connection), which validate
// again. Existing contacts and connections are skipped, never overwritten.
(function() {
    'use strict';

    var FORMAT = 'ratspeak.contacts-connections.v1';
    var MAX_FILE_BYTES = 2 * 1024 * 1024;
    var MAX_CONTACTS = 5000;
    var MAX_CONNECTIONS = 500;
    var NAME_MAX = 64;
    var IFAC_MAX = 128;

    function hasControl(value) {
        for (var i = 0; i < value.length; i++) {
            var code = value.charCodeAt(i);
            if (code < 32 || code === 127) return true;
        }
        return false;
    }

    function validName(value) {
        return typeof value === 'string' && value.length > 0 && value.length <= NAME_MAX &&
            value === value.trim() && !hasControl(value) && !/[\[\]=#]/.test(value);
    }

    function validIfac(value) {
        return typeof value === 'string' && value.length > 0 && value.length <= IFAC_MAX &&
            value === value.trim() && !hasControl(value) && value.indexOf('#') === -1;
    }

    function validHost(host) {
        if (typeof host !== 'string' || !host || host.length > 253) return false;
        if (host.indexOf(':') !== -1) return /^[0-9A-Fa-f:.]+$/.test(host) && host.length >= 2;
        return host.split('.').every(function(label) {
            return /^[A-Za-z0-9_]([A-Za-z0-9_-]{0,61}[A-Za-z0-9_])?$/.test(label);
        });
    }

    function asText(value) {
        return value === undefined || value === null ? '' : String(value);
    }

    // ---- Export ------------------------------------------------------------

    function connectionsFromInterfaces(ifaces, includeSecrets) {
        var out = [];
        [['tcp', 'tcp_client'], ['backbone', 'backbone_client']].forEach(function(pair) {
            ((ifaces && ifaces[pair[1]]) || []).forEach(function(iface) {
                var host = asText(iface.target_host);
                var port = parseInt(asText(iface.target_port), 10);
                if (!host || !(port >= 1 && port <= 65535)) return;
                var name = asText(iface.name);
                var item = { type: pair[0], name: validName(name) ? name : (host + ':' + port),
                    host: host, port: port };
                var ifacName = asText(iface.network_name || iface.networkname);
                var ifacPass = asText(iface.passphrase || iface.pass_phrase);
                var ifacSize = parseInt(asText(iface.ifac_size), 10);
                if (ifacName) item.ifac_name = ifacName;
                if (ifacPass && includeSecrets) item.ifac_pass = ifacPass;
                if ((ifacName || ifacPass) && ifacSize >= 1 && ifacSize <= 64) item.ifac_size = ifacSize;
                if (ifacPass && !includeSecrets) item.ifac_pass_omitted = true;
                out.push(item);
            });
        });
        return out;
    }

    function buildBundle(contacts, ifaces, includeSecrets, now) {
        var bundle = {
            format: FORMAT,
            exported_at: (now || new Date()).toISOString(),
            contacts: [],
            connections: connectionsFromInterfaces(ifaces, !!includeSecrets)
        };
        (contacts || []).forEach(function(contact) {
            var hash = asText(contact && contact.hash).toLowerCase();
            if (!/^[0-9a-f]{16,64}$/.test(hash)) return;
            var item = { hash: hash };
            var name = asText(contact.display_name).trim();
            if (name) item.name = name.slice(0, NAME_MAX);
            bundle.contacts.push(item);
        });
        return bundle;
    }

    function hasPassphrases(ifaces) {
        return connectionsFromInterfaces(ifaces, true).some(function(item) { return !!item.ifac_pass; });
    }

    // ---- Import: strict validation ----------------------------------------

    // Returns { contacts, connections, rejected } or throws an Error whose
    // message is safe to show.
    function parseBundle(raw) {
        if (typeof raw !== 'string' || !raw.trim()) throw new Error('The file is empty');
        if (raw.length > MAX_FILE_BYTES) throw new Error('The file is too large');
        var data;
        try { data = JSON.parse(raw); } catch (_) { throw new Error('This is not a Ratspeak contacts file'); }
        if (!data || typeof data !== 'object' || data.format !== FORMAT) {
            throw new Error('This is not a Ratspeak contacts and connections file');
        }
        var contactsIn = data.contacts === undefined ? [] : data.contacts;
        var connectionsIn = data.connections === undefined ? [] : data.connections;
        if (!Array.isArray(contactsIn) || !Array.isArray(connectionsIn)) {
            throw new Error('The file has an unsupported layout');
        }
        if (contactsIn.length > MAX_CONTACTS || connectionsIn.length > MAX_CONNECTIONS) {
            throw new Error('The file has too many entries');
        }
        var rejected = 0;
        var seenContacts = Object.create(null);
        var contacts = [];
        contactsIn.forEach(function(item) {
            var hash = item && typeof item.hash === 'string' ? item.hash.toLowerCase() : '';
            if (!/^[0-9a-f]{16,64}$/.test(hash) || seenContacts[hash]) { rejected++; return; }
            seenContacts[hash] = true;
            var name = item.name;
            if (name !== undefined && (typeof name !== 'string' || name.length > NAME_MAX || hasControl(name))) {
                rejected++;
                return;
            }
            contacts.push({ hash: hash, name: name ? name.trim() : '' });
        });
        var connections = [];
        connectionsIn.forEach(function(item) {
            var ok = item && (item.type === 'tcp' || item.type === 'backbone') &&
                validHost(item.host) && Number.isInteger(item.port) && item.port >= 1 && item.port <= 65535 &&
                (item.name === undefined || validName(item.name)) &&
                (item.ifac_name === undefined || validIfac(item.ifac_name)) &&
                (item.ifac_pass === undefined || validIfac(item.ifac_pass)) &&
                (item.ifac_size === undefined || (Number.isInteger(item.ifac_size) && item.ifac_size >= 1 && item.ifac_size <= 64));
            if (!ok) { rejected++; return; }
            connections.push({
                type: item.type, host: item.host, port: item.port,
                name: item.name || (item.host + ':' + item.port),
                ifac_name: item.ifac_name || '', ifac_pass: item.ifac_pass || '',
                ifac_size: item.ifac_size, passphraseOmitted: item.ifac_pass_omitted === true
            });
        });
        return { contacts: contacts, connections: connections, rejected: rejected };
    }

    // ---- Import: what would change ----------------------------------------

    function endpointKey(host, port) { return String(host).toLowerCase() + ':' + port; }

    function planImport(bundle, existingContacts, ifaces) {
        var haveContacts = Object.create(null);
        (existingContacts || []).forEach(function(contact) {
            haveContacts[asText(contact && contact.hash).toLowerCase()] = true;
        });
        var plan = { contacts: [], skippedContacts: 0, connections: [], skippedConnections: 0 };
        bundle.contacts.forEach(function(contact) {
            if (haveContacts[contact.hash]) plan.skippedContacts++;
            else plan.contacts.push(contact);
        });

        var haveEndpoints = Object.create(null);
        var usedNames = Object.create(null);
        Object.keys(ifaces || {}).forEach(function(group) {
            if (!Array.isArray(ifaces[group])) return;
            ifaces[group].forEach(function(iface) {
                if (iface && iface.name) usedNames[String(iface.name).toLowerCase()] = true;
                if ((group === 'tcp_client' || group === 'backbone_client') && iface.target_host) {
                    haveEndpoints[endpointKey(iface.target_host, parseInt(asText(iface.target_port), 10))] = true;
                }
            });
        });
        bundle.connections.forEach(function(conn) {
            var key = endpointKey(conn.host, conn.port);
            if (haveEndpoints[key]) { plan.skippedConnections++; return; }
            haveEndpoints[key] = true;
            var name = conn.name, n = 2;
            while (usedNames[name.toLowerCase()]) {
                var suffix = ' (' + n++ + ')';
                name = conn.name.slice(0, NAME_MAX - suffix.length) + suffix;
            }
            usedNames[name.toLowerCase()] = true;
            plan.connections.push(Object.assign({}, conn, { name: name }));
        });
        return plan;
    }

    function connectionArgs(conn) {
        var hasIfac = !!(conn.ifac_name || conn.ifac_pass);
        var args = { host: conn.host, port: conn.port, name: conn.name,
            ifac_enabled: hasIfac, ifac_network_name: conn.ifac_name, ifac_passphrase: conn.ifac_pass };
        if (hasIfac && conn.ifac_size !== undefined) args.ifac_size = conn.ifac_size;
        return args;
    }

    // One item at a time: each backend command validates and writes config.
    function runImport(plan, invoke, onProgress) {
        var summary = { contactsAdded: 0, connectionsAdded: 0, failed: [] };
        var total = plan.contacts.length + plan.connections.length;
        var done = 0;
        function step(label) { done++; if (onProgress) onProgress(done, total, label); }
        function errorText(err) { return (err && err.message) || String(err || 'failed'); }
        var chain = Promise.resolve();
        plan.contacts.forEach(function(contact) {
            chain = chain.then(function() {
                var args = { hash: contact.hash };
                if (contact.name) args.display_name = contact.name;
                return invoke('add_contact', { args: args }).then(function() {
                    summary.contactsAdded++;
                }, function(err) {
                    summary.failed.push((contact.name || contact.hash.slice(0, 12)) + ': ' + errorText(err));
                }).then(function() { step(contact.name || contact.hash.slice(0, 12)); });
            });
        });
        plan.connections.forEach(function(conn) {
            chain = chain.then(function() {
                var command = conn.type === 'backbone' ? 'add_backbone_connection' : 'add_tcp_connection';
                return invoke(command, { args: connectionArgs(conn) }).then(function() {
                    summary.connectionsAdded++;
                }, function(err) {
                    summary.failed.push(conn.name + ': ' + errorText(err));
                }).then(function() { step(conn.name); });
            });
        });
        return chain.then(function() { return summary; });
    }

    // ---- UI ----------------------------------------------------------------

    function pad(n) { return (n < 10 ? '0' : '') + n; }

    function exportContactsAndConnections() {
        Promise.all([
            RS.invoke('api_contacts'),
            RS.invoke('api_hub_interfaces')
        ]).then(function(results) {
            var contacts = results[0] || [];
            var ifaces = results[1] || {};
            var connectionCount = connectionsFromInterfaces(ifaces, false).length;
            if (!contacts.length && !connectionCount) {
                showToast('There are no contacts or connections to export', 'toast-info', 3000);
                return null;
            }
            var message = 'Contacts: ' + contacts.length + '. Connections: ' + connectionCount +
                '. The file is not encrypted.';
            if (hasPassphrases(ifaces)) {
                return rsConfirmWithCheckbox({
                    title: 'Export contacts and connections',
                    message: message,
                    confirmText: 'Export',
                    checkboxLabel: 'Include IFAC passphrases',
                    checkboxHelp: 'Anyone who has the file can read them. Without them, connections are imported without their IFAC passphrase.',
                    defaultChecked: false
                }).then(function(answer) {
                    return answer && answer.confirmed ? { contacts: contacts, ifaces: ifaces, secrets: !!answer.checked } : null;
                });
            }
            return rsConfirm({ title: 'Export contacts and connections', message: message, confirmText: 'Export' })
                .then(function(ok) { return ok ? { contacts: contacts, ifaces: ifaces, secrets: false } : null; });
        }).then(function(choice) {
            if (!choice) return;
            var now = new Date();
            var bundle = buildBundle(choice.contacts, choice.ifaces, choice.secrets, now);
            var bytes = new TextEncoder().encode(JSON.stringify(bundle, null, 2));
            var fileName = 'ratspeak-contacts-connections-' + now.getFullYear() + pad(now.getMonth() + 1) +
                pad(now.getDate()) + '.json';
            return saveBytesToUserFile(bytes, fileName, 'application/json').then(function(result) {
                if (!result) return;
                showToast(result.method === 'android-document'
                    ? 'Contacts and connections saved'
                    : 'Save prompt opened for ' + result.fileName, 'toast-success', 3500);
            });
        }).catch(function(err) {
            showToast((err && err.message) || 'Could not export contacts and connections', 'toast-error', 4000);
        });
    }

    function importContactsAndConnections() {
        var input = document.getElementById('contacts-backup-file-input');
        if (!input) return;
        input.value = '';
        input.click();
    }

    function handleImportFile(file) {
        if (!file) return;
        if (file.size > MAX_FILE_BYTES) {
            showToast('The file is too large', 'toast-error', 3000);
            return;
        }
        var bundle;
        file.text().then(function(raw) {
            bundle = parseBundle(raw);
            return Promise.all([RS.invoke('api_contacts'), RS.invoke('api_hub_interfaces')]);
        }).then(function(results) {
            var plan = planImport(bundle, results[0] || [], results[1] || {});
            if (!plan.contacts.length && !plan.connections.length) {
                showToast('Nothing new to import: everything in the file is already here', 'toast-info', 4000);
                return null;
            }
            var message = 'Add ' + plan.contacts.length + ' contacts and ' + plan.connections.length +
                ' connections? Entries that already exist are skipped.';
            return rsConfirm({ title: 'Import contacts and connections', message: message,
                confirmText: 'Import' }).then(function(ok) { return ok ? plan : null; });
        }).then(function(plan) {
            if (!plan) return;
            var progress = rsProgress({ title: 'Importing…', message: 'Preparing…' });
            return runImport(plan, RS.invoke, function(done, total, label) {
                progress.update(done + ' / ' + total + ': ' + label);
            }).then(function(summary) {
                progress.close();
                if (typeof refreshConfigInterfaces === 'function') refreshConfigInterfaces();
                showToast('Added ' + summary.contactsAdded + ' contacts and ' + summary.connectionsAdded +
                    ' connections', summary.failed.length ? 'toast-warning' : 'toast-success', 4000);
                if (summary.failed.length) {
                    var shown = summary.failed.slice(0, 5).join('; ');
                    var hidden = summary.failed.length - 5;
                    rsAlert({ title: 'Import finished with errors',
                        message: 'Not added: ' + shown + (hidden > 0 ? ' (+' + hidden + ')' : '') });
                }
            });
        }).catch(function(err) {
            showToast((err && err.message) || 'Could not import this file', 'toast-error', 4000);
        });
    }

    function bind() {
        var exportBtn = document.getElementById('settings-export-contacts-btn');
        var importBtn = document.getElementById('settings-import-contacts-btn');
        var input = document.getElementById('contacts-backup-file-input');
        if (exportBtn) exportBtn.addEventListener('click', exportContactsAndConnections);
        if (importBtn) importBtn.addEventListener('click', importContactsAndConnections);
        if (input) input.addEventListener('change', function() { handleImportFile(input.files && input.files[0]); });
    }

    window.RS = window.RS || {};
    RS.contactsBackup = { FORMAT: FORMAT, buildBundle: buildBundle, parseBundle: parseBundle,
        planImport: planImport, runImport: runImport, connectionArgs: connectionArgs };
    if (typeof document !== 'undefined' && document.addEventListener) {
        if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', bind);
        else bind();
    }
})();
