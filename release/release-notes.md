## v1.0.35

### Added

- Added Auto-Lock for passcode-protected identities.
- Added a setting for how much notifications show.

### Fixed and improved

- Improved Bluetooth on Android: radios reconnect faster and show why they can't connect.
- Fixed pairing a radio again after removing it from Android's Bluetooth settings.
- Radios that reject a setting now say which one.
- Improved identity switching, so messages and contacts always stay with the right identity.
- Showing your recovery phrase or exporting a key now asks for your PIN, and copied phrases clear after a minute.
- Fixed a file name bug when exporting on desktop.
- Fixed recovery phrases not being saved in rare cases.
- Improved compatibility with the latest Python Reticulum and LXMF.
- Fixed downloading messages from propagation nodes that hold many messages.
- Fixed game boards being cut off on phones; games now open full screen.
- Made captured chess pieces and game results easier to see.

### Build and compatibility

- Linux: USB radios are now available only to the signed-in user. The rule is now `70-ratspeak.rules`; delete any old `99-ratspeak.rules` you copied by hand.
