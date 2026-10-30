# Changelog of the Ratspeak-ru fork

Changes of the fork relative to upstream Ratspeak. Upstream's own history is in [CHANGELOG.md](CHANGELOG.md).

## Unreleased

- Fork code lives in fork-only files; edits to upstream files are limited to re-appliable hooks (`fork/hooks.json`), and `fork/sync_upstream.py` merges the official Ratspeak without manual conflict resolution. See `fork/README.md`.
- Russian interface layer (`dashboard/static/locales/ru.*`) with plural-aware templates and `tools/i18n` for maintaining it or adding languages.
- Connection links `ratspeak://tcp` and `ratspeak://radio` that pre-fill Network → Connections → Internet/TCP or Radio, with optional IFAC network name and passphrase. A link never connects by itself. A link shared as plain text through the Android share sheet opens the same form. See `docs/connection-links.md` and `tools/connection_link.py`.
- Share QR code in the connection menu for Internet/TCP and Radio connections.
- Export and import of contacts and Internet/TCP connections in Settings → Identity. IFAC passphrases are included only on request; the file is not encrypted.
- Announce propagation radar in Network → Activity: hops as rings, interfaces as sectors, destinations as dots, with waves for new announces.
- Морской бой (Battleship): игра для двоих с честной проверкой флотов (фиксация SHA-256 до боя, раскрытие в конце). Работает только между клиентами форка. См. `docs/battleship.md`.
- Peer view modes on the home screen (default, by status, by contacts, compact list, tiles); the Recent Peers panel is a spoiler that is closed by default.
- `lxma://` addresses: parser, native deep-link filters, an Add contact form, and clickable addresses in message text; a known contact opens its conversation. See `docs/connection-links.md`.
- Location (GNSS) setting with a phone or radio position source (`ru_geo.js`). Nothing is sent by it.
- Automatic fork versions `<upstream version>-ru.N` and GitHub Releases with debug-signed `.apk` files (`fork-release.yml`). See `docs/fork-versioning.md`.
- Action log of the fork: `docs/FORK-LOG.md`.
- Route to a contact: a Route action in the conversation menu and the contact sheet shows the known path (hops, interface, next hop, time left) and can request it again from the network.
