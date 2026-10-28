// Pre-fill the add-connection forms from ratspeak://tcp and ratspeak://radio links.
// Fork-only file. It uses the form helpers modals.js already defines
// (openConnectModal, setConnectTab, openRnodeModal, ...) at call time, so modals.js stays untouched.
(function() {
    'use strict';

    // ratspeak://tcp and ratspeak://radio deep links. Rust has already validated the
    // typed target; this only pre-fills the normal add-connection form. A link never
    // connects by itself: the user reviews the details and confirms.
    function _connectionLinkInt(value, min, max) {
        return typeof value === 'number' && Number.isInteger(value) &&
            value >= min && value <= max;
    }

    function _connectionLinkOptionalText(value) {
        return typeof value === 'string' ? value : '';
    }

    function _openTcpConnectionLink(target) {
        if (typeof target.host !== 'string' || !target.host ||
                !_connectionLinkInt(target.port, 1, 65535)) return false;
        openConnectModal();
        // Custom tab first: switching to it clears the name field.
        setConnectTab('custom');
        document.getElementById('connect-host').value = target.host;
        document.getElementById('connect-port').value = String(target.port);
        var name = _connectionLinkOptionalText(target.name);
        var nameEl = document.getElementById('connect-name');
        var nameField = document.getElementById('connect-name-field');
        if (nameEl) nameEl.value = name;
        if (nameField) nameField.style.display = name ? '' : 'none';
        var ifacName = _connectionLinkOptionalText(target.ifac_network_name);
        var ifacPass = _connectionLinkOptionalText(target.ifac_passphrase);
        var hasIfac = !!(ifacName || ifacPass);
        var ifacCheckbox = document.getElementById('connect-use-ifac');
        var ifacNameEl = document.getElementById('connect-ifac-network-name');
        var ifacPassEl = document.getElementById('connect-ifac-passphrase');
        var ifacSizeEl = document.getElementById('connect-ifac-size');
        if (ifacCheckbox) ifacCheckbox.checked = hasIfac;
        if (ifacNameEl) ifacNameEl.value = ifacName;
        if (ifacPassEl) ifacPassEl.value = ifacPass;
        if (ifacSizeEl) ifacSizeEl.value = '';
        _ifacSyncFields('connect');
        showToast('Review the connection details, then press Connect', 'toast-info', 5000);
        return true;
    }

    function _openRadioConnectionLink(target) {
        var airtimeShort = typeof target.airtime_limit_short === 'number' ? target.airtime_limit_short : null;
        var airtimeLong = typeof target.airtime_limit_long === 'number' ? target.airtime_limit_long : null;
        if (!_connectionLinkInt(target.frequency, 1, 3000000000) ||
                !_connectionLinkInt(target.bandwidth, 1, 2000000) ||
                !_connectionLinkInt(target.spreading_factor, 5, 12) ||
                !_connectionLinkInt(target.coding_rate, 5, 8) ||
                !_connectionLinkInt(target.tx_power, 0, 37)) return false;
        var custom = target.custom_params === true;
        if (!custom && (typeof target.region_key !== 'string' ||
                typeof target.preset_key !== 'string')) return false;
        // The device (BLE/USB/serial/TCP) is local to this phone or computer and
        // is never part of a link: open on the device step.
        openRnodeModal('ble');
        var apply = function() {
            _rnodeApplyRadioControls(
                custom ? _RNODE_CUSTOM_REGION_KEY : target.region_key,
                custom ? _RNODE_CUSTOM_PRESET_KEY : target.preset_key,
                target.frequency,
                target.bandwidth,
                target.spreading_factor,
                target.coding_rate,
                target.tx_power,
                custom || airtimeShort !== null || airtimeLong !== null
            );
            _rnodeSetAirtimeLimits(airtimeShort, airtimeLong);
            if (typeof target.mode === 'string' && target.mode) _rnodeSetInterfaceMode(target.mode);
            var nameEl = document.getElementById('rnode-iface-name');
            var name = _connectionLinkOptionalText(target.name);
            if (nameEl && name) nameEl.value = name;
        };
        // Registered after openRnodeModal's own default-controls callback, so the
        // link's parameters win once the preset catalog is available.
        loadRnodePresetCatalog().then(apply).catch(function() {});
        showToast('Select your radio device, then review the settings', 'toast-info', 5000);
        return true;
    }

    function openNativeConnectionLink(target) {
        if (!target || typeof target !== 'object') return false;
        if (target.kind === 'tcp') return _openTcpConnectionLink(target);
        if (target.kind === 'radio') return _openRadioConnectionLink(target);
        return false;
    }
    window.openNativeConnectionLink = openNativeConnectionLink;

})();
