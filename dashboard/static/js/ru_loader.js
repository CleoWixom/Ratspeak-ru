// Ratspeak-ru loader: the one fork hook in index.html. It adds the fork's
// stylesheet, the few UI blocks that live inside upstream screens, and the fork's
// scripts, so upstream HTML, CSS and JS files stay untouched and easy to merge.
(function() {
    'use strict';

    var TAG = 'ratspeak-ru';

    // Diagnostics go through RS.diag (silent unless enabled), not straight to the browser log.
    function warn() {
        var rs = window.RS;
        if (rs && typeof rs.diag === 'function') {
            rs.diag.apply(rs, ['warn'].concat([].slice.call(arguments)));
        }
    }
    var query = (document.currentScript && document.currentScript.src.split('?')[1]) || '';
    function versioned(path) { return query ? path + '?' + query : path; }

    var SCRIPTS = [
        '/static/js/connection_link_presenters.js',
        '/static/js/native_connection_link.js',
        '/static/js/connection_qr.js',
        '/static/js/contacts_backup.js',
        '/static/js/announce_radar.js',
        '/static/js/route_lookup.js',
        '/static/js/ru_text_scale.js',
        '/static/js/ru_extensions.js'
    ];

    var SETTINGS_ROWS =
        '<div class="settings-row" id="ru-export-contacts-row">' +
            '<div class="settings-row-info">' +
                '<span class="settings-row-label">Export Contacts & Connections</span>' +
                '<span class="settings-row-desc">Save your contacts and all Internet/TCP connections to a file.</span>' +
            '</div>' +
            '<button class="selector-badge selector-badge-no-caret" id="settings-export-contacts-btn">Export</button>' +
        '</div>' +
        '<div class="settings-row" id="ru-import-contacts-row">' +
            '<div class="settings-row-info">' +
                '<span class="settings-row-label">Import Contacts & Connections</span>' +
                '<span class="settings-row-desc">Add contacts and Internet/TCP connections from a file. Existing ones are kept.</span>' +
            '</div>' +
            '<button class="selector-badge selector-badge-no-caret" id="settings-import-contacts-btn">Import</button>' +
        '</div>';

    var RADAR_CARD =
        '<div class="announce-radar-card" id="announce-radar-card">' +
            '<div class="announce-radar-header" role="button" tabindex="0" aria-expanded="true">' +
                '<svg class="announce-radar-icon" viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="2"/><path d="M16.2 7.8a6 6 0 0 1 0 8.4"/><path d="M7.8 16.2a6 6 0 0 1 0-8.4"/><path d="M19 5a10 10 0 0 1 0 14"/><path d="M5 19a10 10 0 0 1 0-14"/></svg>' +
                '<span class="announce-radar-title">Announce propagation</span>' +
                '<span class="announce-radar-summary" id="announce-radar-summary">0</span>' +
                '<svg class="announce-radar-chevron" viewBox="0 0 24 24" width="14" height="14" aria-hidden="true"><polyline points="6 9 12 15 18 9"/></svg>' +
            '</div>' +
            '<div class="announce-radar-body">' +
                '<canvas id="announce-radar-canvas" class="announce-radar-canvas" aria-label="Announce propagation radar"></canvas>' +
                '<div class="announce-radar-tip inline-hint" id="announce-radar-tip">Tap a dot to see its details.</div>' +
                '<div class="announce-radar-legend" id="announce-radar-legend"></div>' +
                '<div class="announce-radar-rate inline-hint">Announces in the last minute: <span id="announce-radar-rate">0</span></div>' +
            '</div>' +
        '</div>';

    function textScalePreset(v) {
        return '<label class="settings-type-preset" data-scale="' + v + '">' +
            '<input type="radio" name="settings-text-scale" value="' + v + '">' +
            '<span class="settings-type-preset-card">' +
                '<span class="settings-type-preset-sample" aria-hidden="true">Aa</span>' +
                '<span class="settings-type-preset-value">' + v + '%</span>' +
            '</span></label>';
    }

    function injectStyles() {
        if (document.getElementById('ratspeak-ru-styles')) return;
        var link = document.createElement('link');
        link.id = 'ratspeak-ru-styles';
        link.rel = 'stylesheet';
        link.href = versioned('/static/css/ratspeak-ru.css');
        document.head.appendChild(link);
    }

    // Each block is skipped (with a console note) when its anchor is missing, so a
    // reworked upstream screen costs one feature instead of the whole app.
    function injectUi() {
        var backup = document.getElementById('settings-backup-identity-btn');
        var row = backup && backup.closest('.settings-row');
        if (row && !document.getElementById('settings-export-contacts-btn')) {
            row.insertAdjacentHTML('afterend', SETTINGS_ROWS);
        } else if (!row) {
            warn(TAG, 'missing-anchor', 'settings-backup-identity-btn');
        }

        // Text size 50/70/90%: added before upstream's DOMContentLoaded handler binds the radios.
        var first = document.querySelector('input[name="settings-text-scale"][value="100"]');
        var firstLabel = first && first.closest('.settings-type-preset');
        if (firstLabel && !document.querySelector('input[name="settings-text-scale"][value="50"]')) {
            firstLabel.insertAdjacentHTML('beforebegin', textScalePreset(50) + textScalePreset(70) + textScalePreset(90));
        } else if (!firstLabel) {
            warn(TAG, 'missing-anchor', 'settings-text-scale');
        }

        var activity = document.getElementById('network-activity');
        if (activity && !document.getElementById('announce-radar-card')) {
            activity.insertAdjacentHTML('afterbegin', RADAR_CARD);
        } else if (!activity) {
            warn(TAG, 'missing-anchor', 'network-activity');
        }

        if (!document.getElementById('contacts-backup-file-input')) {
            var input = document.createElement('input');
            input.type = 'file';
            input.id = 'contacts-backup-file-input';
            input.accept = '.json,application/json,text/plain';
            input.style.display = 'none';
            document.body.appendChild(input);
        }
    }

    function loadScripts(index) {
        if (index >= SCRIPTS.length) return;
        var script = document.createElement('script');
        script.src = versioned(SCRIPTS[index]);
        script.onload = function() { loadScripts(index + 1); };
        script.onerror = function() {
            warn(TAG, 'load-failed', SCRIPTS[index]);
            loadScripts(index + 1);
        };
        document.body.appendChild(script);
    }

    injectStyles();
    injectUi();
    loadScripts(0);
})();
