# Changelog

This changelog records user-visible Ratspeak changes and release-engineering
changes that materially affect how an artifact is reproduced.

## [Unreleased]

### Fixed and improved

- Bluetooth radios that are off, out of range or need pairing again now say why, with Connection Help, instead of always showing "Waiting for radio".
- A radio that rejects its settings, such as TX power below an amplifier's minimum, stops with the setting named and an Edit action, on Bluetooth, USB, serial and TCP.
- Resuming or editing a radio that is away keeps it enabled and connects when it returns, instead of pausing or reverting it after two minutes.
- Desktop serial and TCP radios that are unplugged or busy keep retrying and show why: radio not found, port access denied or port in use.
- Android reconnects to a returning Bluetooth radio within seconds using background auto-connect, instead of retrying as rarely as every 10 minutes.
- Fixed Android Bluetooth radio sessions dropping when the app reconnected slowly.
- Fixed propagation pickup and purge when many messages are stored on a relay.
- Preserved unknown interface locations and restored discovery announcements.
- Bounded background announcements at flow-controlled radio writers.
- Ratspeak warns at startup when its data is on a network drive or in temporary memory, where messages and keys may not survive a crash or restart.
- Deleting all messages or contacts now clears them from every screen at once, and blocked users stay blocked.
- Creating, importing or restoring an identity no longer fails at random while the app is busy, and reading a channel or bringing the app forward no longer drops an announce.
- A failed identity switch or security key activation brings the previous identity back online and shows why, instead of leaving the app offline mid-switch.
- Removing the active hardware identity, or starting with no identity selected, opens the most recently used one that still opens; a protected one asks for its PIN.
- Messages and game moves received during an identity switch stay with the identity they were sent to and never show under the next one.
- After an identity switch the new identity's contacts show at once, instead of an empty contact list until the view is reopened.
- Renames, status changes and blocks made during an identity switch are refused instead of landing on the next identity, and a rename never announces the old name afterwards. A network block or unblock that a switch interrupts completes once its identity runs again.
- Ratspeak won't open data that another Ratspeak app is using or that a newer version saved, and says what to do instead. Drives that can't lock get a warning.
- Opportunistic messages use the whole encrypted packet, as in Python LXMF, and leave room for a stamp only when one is attached; larger messages are sent and shown as Direct.
- A PIN-protected identity asks for its PIN again after you switch to another identity and back.
- If the identity that takes over after removing a hardware identity can't load, Ratspeak leaves no identity active instead of one without a network.
- An identity that can't load no longer leaves Ratspeak looking ready without messaging, and a failed switch to it no longer makes it the identity a later start opens.
- Auto-Lock now locks passcode identities too, and counts time the device slept, so a lock that fell due during sleep happens within seconds of waking. If you turned on the hardware auto-lock, passcode identities now lock after the same time.
- Auto-Lock locks an identity even when its network is stopped or failed to start, and adding a PIN to the open identity starts its Auto-Lock.
- While an identity is locked, its messages, contacts and other data stay out of the app until you unlock it.
- Resetting, reprovisioning or restoring the security key of the active hardware identity keeps that identity locked, instead of opening its data without a PIN.
- On the desktop, saving an identity export or backup says whether the file was saved, instead of only that the save prompt opened.
- Ratspeak keeps its data folder and desktop diagnostic logs private to your user account, and copying the shared-instance access configuration asks for your identity's PIN, or a confirmation when it has none.
- The Linux packages' USB radio rule gives access to the user signed in at the computer, and applies to radios that are already plugged in.
- Notification previews can show the sender and message, only the sender, or neither; a call always shows who is calling. On Android and iOS, switching identities or a lock removes the identity's alerts, and a call that rings during a switch no longer alerts under the next identity.
- Creating, importing or restoring an identity with a recovery phrase saves the phrase first. If it can't be saved, Ratspeak says so and creates no identity, instead of one whose phrase can never be shown again.
- Showing the recovery phrase or exporting an identity's key or backup asks for the identity's PIN, or a confirmation when it has none, and works while the identity is locked; backups of PIN-protected identities keep their recovery phrase. Copied recovery phrases and access configurations clear from the clipboard after a minute and stay out of clipboard history where the system allows.

### Build and compatibility

- Updated the coordinated source graph to rsReticulum 1.3.0 (`~1.3.0`), including
  the nullable discovery coordinate API migration.

## [1.0.34] - 2026-10-10

### Added

- Added voice messages for compatible handhelds, with the recording format selected automatically.
- Added Codec2 support for compatible low-bandwidth voice calls.

### Fixed and improved

- Improved Windows Bluetooth discovery and reconnection, including saved devices missing from scan results.
- Fixed Android Bluetooth cleanup when permissions are revoked.
- Kept recorded clips available for retry and prevented duplicate voice messages after an uncertain send result.
- Improved recording limits, voice controls, and handling when leaving a chat or changing identities.
- Removed legacy text fallbacks for message reactions.

### Build and compatibility

- Source builds require Rust 1.89 or later.
- Compact recordings are limited to 15 seconds. Receive the handheld's announce before recording so the app can select its compatible format.

### Known issue

- Some attachments sent to NomadNet may appear delivered without being saved by the receiver due to an upstream Python Reticulum issue.

## [1.0.33] - 2026-09-24

### Fixed and improved

- Fixed “Attachment chunk is too large” errors when sending large files or photos.
- Kept automatically prepared photos within their size limit while preserving manual size choices.
- Fixed attachment replacement and cancellation, long or Unicode filenames, and simultaneous large-image previews.
- Improved Bluetooth Peer transfers and prevented timeouts while data is still moving.
- Corrected file transfer progress and reduced repetitive Activity entries.
- Prevented chats from jumping when images load, and kept recent messages visible when the iPhone keyboard opens.
- Removed stale startup warnings after a radio interface is removed or paused.
- Improved first-message delivery to handhelds and selected compression based on the receiving peer’s capabilities.
- Improved discovery on busy or slow connections, reducing the need for manual announces.

### Known issue

- Some attachments sent to NomadNet may appear delivered without being saved by the receiver due to an upstream Python Reticulum issue.


## [1.0.32] - 2026-09-19

### Added

- Added Android system sharing for text, links and single photos, with recent-chat
  and contact selection, photo sizing, protected drafts, and an explicit Send action.
- Added Developer Mode network settings for choosing a Ratspeak-managed stack
  or an authenticated existing local Reticulum instance.

### Fixed

- Prevented telemetry-only LXMF updates from creating empty chat messages,
  unread counts or notifications; preserved title-only messages and made
  unavailable attachments explicit.
- Fixed Windows System color mode so it follows operating-system changes
  without remaining locked to the previous Light or Dark choice.
- Refined mobile composer alignment, Network Ownership controls, Bluetooth
  progress sheets, and dismissible radio startup warnings.
- Simplified share recipient selection and dismissed the keyboard before
  photo sizing.
- Corrected same-radio relaying, bounded discovery retries and slow-link timing.
  Failed-route recovery preserves freshly learned local routes and supports
  bounded authenticated recovery through an existing Python stack. Legacy
  Python owners reset by destination; this is not an atomic remote-route check.
- Separated local send-capacity waits from delivery-proof timeouts, allowing
  failed Links and stalled attachments to release following messages for retry
  while healthy slow transfers retain their protocol-owned windows.
- Kept cancellation and delayed completion tied to their original messages,
  and retained attachment memory reservations through inbound processing.
  Cancellation cannot recall bytes already accepted by a driver.
- Corrected Android USB radio readiness and late write-completion accounting
  during reconnects.
- Corrected propagation-node discovery metadata and preserved stored offline
  messages across node restarts, including compatible recovery of older
  unstamped storage.
- Improved voice-preview startup, duration and recovery handling, preserved paused
  seeking, and bounded playback cleanup before retries or recording.
- Retained the Android runtime across task removal and Activity recreation,
  while fencing stale native callbacks and preserving explicit-stop behavior.
- Prevented stale search, conversation and contact results from replacing newer
  navigation or identity state; improved tablet typing, pickers and keyboard access.
- Protected network credentials and ownership transitions, and prevented delayed
  access imports from overwriting manually edited settings.

### Build

- Enforced the pinned Android NDK, release JNI boundaries and 16 KiB native
  packaging alignment; excluded dashboard test scripts from embedded assets.
- Added exact dependency-set source reconstruction and packaging guidance for
  reproducible downstream builds.
- Aligned networking dependencies across standalone core and packaged builds,
  including corrected Windows socket ownership and Wine compatibility.

### Known limitations

- Known interoperability limitation: some attachment transfers to NomadNet can
  appear delivered without being stored by the receiver due to an upstream
  Python Reticulum completion-state defect.

## [1.0.31] - 2026-08-26

### Fixed

- Prevented app-managed shared Reticulum instances on Linux and Android from
  colliding with other local instances that use a different port, while
  preserving explicit operator configurations.
- Restored mobile keyboard avoidance across Direct Messages, Channels, and
  input sheets, and made first-run setup dismiss the keyboard after submission.
- Enabled iOS notification sounds when permitted by the user's device settings.
- Hardened voice messages across Android versions, including short or silent
  recordings, first-time microphone permission, review and discard ownership,
  correct re-recorded previews, and visible waveform playback progress.
- Aligned Channel receive-window handling with Reticulum 1.4.2 so far-future
  packets are rejected until a retransmission enters the valid window.

## [1.0.30] - 2026-08-24

### Fixed

- Restored mobile message actions on text, links, and media while keeping
  deliberate native text selection available from an elevated message, and
  improved the corresponding desktop mouse and keyboard interactions.
- Made previously paired Bluetooth RNodes advertise automatically after a
  power cycle and reconnect without requiring pairing mode again.
- Made Android RNode recovery show accurate waiting, connecting, initializing,
  and connected states without replacing an active retry, and fixed the
  pairing sheet jumping when the system PIN prompt closes.

## [1.0.29] - 2026-08-21

### Fixed

- Qualified and fixed Bluetooth RNode support on iOS and Android for Heltec
  V3/V4 and LilyGO T114/T-Echo, including fresh pairing, adapter toggles,
  walking out of range, automatic reconnect, and queued-message recovery.
- Made RNode readiness reflect the completed hardware handshake immediately,
  prevented traffic loss across reconnect generations, and used each device's
  advertised four-character identifier in its default interface name.
- Made manual and interface-online announces coalesce into one prompt queue
  operation, report actual interface acceptance, and remain responsive during
  background maintenance.
- Moved LXMF identity, ratchet, and router persistence off the protocol lock
  and made pruning incremental, durable, and safe under concurrent activity.
- Aligned LXMF first-hop establishment timing with Reticulum so slower radio
  links are not failed prematurely.
- Clarified pending-message cancellation as stopping local retries without
  promising that a copy already handed to the network can be recalled.
- Enabled native selection and copying of sent and received message text, and
  applied the same desktop typing-assistance policy to Direct Messages and
  Channels.

### Changed

- Added an exact dependency manifest and coordinated annotated sibling tags
  for reproducible v1.0.29 builds without date-based revision guessing.

## [1.0.28] - 2026-08-18

### Fixed

- Fixed RNode startup compatibility by treating optional EEPROM information as
  advisory instead of rejecting otherwise usable hardware, including the T114.
- Restored reliable BLE transmission after an RNode connects.
- Fixed Announce sometimes staying queued even while an external RNode was
  connected and ready to transmit.
- Fixed Linux AppImage startup crashes caused by incompatible system
  AppIndicator and GLib libraries.
- Simplified interface status messages so connection and hardware warnings are
  shown as plain yellow text instead of pill-shaped badges.

## [1.0.27] - 2026-08-17

### Changed

- Classified every Rust package as application internal and added pinned,
  CI-enforced API snapshots to protect coordinated workspace changes without
  presenting Ratspeak as a public Rust SDK.
- Preserved and now inspects every name-based Android Rust/Kotlin JNI boundary
  in minified release artifacts, restoring BLE RNode, USB permission, platform
  replay, native file save, and call/voice-message audio entry points that R8
  could rename or remove beginning with `1.0.26d`.
- Adopted upstream `opus-rs` 0.1.29 through rsLXST with heap-backed codec
  state, raised the unified first-party MSRV to Rust 1.87, retained Android
  ARMv7 voice support, and made Android i686 explicitly unsupported.
- Qualified the `1.0.27` source with one reviewed, exact component graph for
  CI and every platform release workflow: rsReticulum/rsLXMF 1.2, rsLXST 0.2,
  and lrgp 0.4.1.
- Added platform-scoped source BOM generation with both Cargo lockfile hashes,
  exact component commits, component compatibility versions, declared release
  toolchains, and runner-observed tool versions.
- Pinned the Node runtime used to qualify release source and emit BOM evidence.
- Made the Android and iOS build numbers monotonic and independent of the
  user-visible marketing version.
- Declared every first-party Ratspeak package non-publishable and recorded
  compatible versions alongside all sibling path dependencies.

[Unreleased]: https://github.com/ratspeak/Ratspeak/compare/v1.0.32...HEAD
[1.0.32]: https://github.com/ratspeak/Ratspeak/compare/v1.0.31...v1.0.32
[1.0.31]: https://github.com/ratspeak/Ratspeak/compare/v1.0.30...v1.0.31
[1.0.30]: https://github.com/ratspeak/Ratspeak/compare/v1.0.29...v1.0.30
[1.0.29]: https://github.com/ratspeak/Ratspeak/compare/v1.0.28...v1.0.29
[1.0.28]: https://github.com/ratspeak/Ratspeak/compare/v1.0.27...v1.0.28
[1.0.27]: https://github.com/ratspeak/Ratspeak/compare/v1.0.26o...v1.0.27
