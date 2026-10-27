<div align="center">

<img src="src-tauri/icons/128x128.png" width="88" height="88" alt="Ratspeak logo">

# Ratspeak-ru

**Неофициальный форк [Ratspeak](https://github.com/ratspeak/Ratspeak): русский интерфейс и дополнительные функции подключения.**<br>
*Unofficial fork of [Ratspeak](https://github.com/ratspeak/Ratspeak): Russian UI and extra connection features.*

Ratspeak is a native desktop and mobile client for E2EE conversations over
Reticulum, a new type of mesh networking. Ratspeak gives you messaging, file/image sharing, voice calls and voice messages (experimental), Channels, LoRa capability, WiFi, BLE, TCP, offline messaging, turn-based games, and more.

[Click here for iOS TestFlight Public Beta!](https://testflight.apple.com/join/AzzW4Jg6)

[Upstream](https://github.com/ratspeak/Ratspeak) |
[Docs](https://docs.ratspeak.org/) |
[Build from source](https://docs.ratspeak.org/docs/reference/building-from-source) |
[rsReticulum](https://github.com/ratspeak/rsReticulum) |
[rsLXMF](https://github.com/ratspeak/rsLXMF) |
[rsLXST](https://github.com/ratspeak/rsLXST)


[![License: AGPL-3.0-or-later](https://img.shields.io/badge/license-AGPL--3.0--or--later-blue.svg)](LICENSE)
[![Rust 1.89+](https://img.shields.io/badge/rust-1.89%2B-orange.svg)](https://www.rust-lang.org)
[![Status](https://img.shields.io/badge/status-alpha-yellow.svg)](#current-state)
[![Fork of ratspeak/Ratspeak](https://img.shields.io/badge/fork%20of-ratspeak%2FRatspeak-lightgrey.svg)](https://github.com/ratspeak/Ratspeak)

<img src="docs/readme/ratspeak-showcase.png" alt="Ratspeak running on desktop and mobile" width="100%">

###### *Note: Ratspeak is currently in ALPHA. If you are looking for a more stable<br> experience, waiting for a later stable release is recommended.*

</div>

## О форке

Этот репозиторий — **форк** [ratspeak/Ratspeak](https://github.com/ratspeak/Ratspeak),
не официальная версия проекта. Авторы оригинала не отвечают за изменения форка;
вопросы по ним задавайте в [Issues этого репозитория](https://github.com/CleoWixom/Ratspeak-ru/issues),
а не в основном проекте. Сетевой стек, шифрование и протоколы не менялись:
форк добавляет перевод и функции вокруг подключений и контактов, поэтому остаётся
совместимым с обычным Ratspeak и остальной сетью Reticulum.

Лицензия прежняя: AGPL-3.0-or-later. Изменения форка распространяются на тех же
условиях, исходный текст открыт в этом репозитории. Установочные пакеты,
TestFlight и сайт ratspeak.org выше относятся к оригиналу и **не содержат**
функций форка. Debug-APK форка собирает workflow *Android debug APK* в GitHub Actions.

### Что добавлено в форке

- **Русский интерфейс.** Перевод работает отдельным слоем и не меняет логику
  приложения: словарь `dashboard/static/locales/ru.*` (более 2500 строк), шаблоны для
  динамических сообщений с правильными окончаниями («1 контакт, 2 контакта, 5 контактов»).
  Язык берётся из системы или из `localStorage['ratspeak_lang']` (`en` отключает перевод).
  Инструменты для обновления перевода и добавления других языков:
  [`tools/i18n`](tools/i18n/README.md).
- **Ссылки подключения (deep link).** `ratspeak://tcp?...` и `ratspeak://radio?...`
  открывают приложение и форму **Сеть → Соединения → Интернет/TCP** или **Радио** с
  заполненными полями: хост, порт, имя, при необходимости имя сети и парольная фраза
  IFAC; для радио частота, полоса, SF, CR, мощность и пресеты. Ссылка ничего не
  подключает сама, пользователь проверяет данные и нажимает «Подключить». На Android ссылка,
  отправленная через системное «Поделиться», открывает ту же форму, а не черновик
  сообщения. Формат и примеры: [docs/connection-links.md](docs/connection-links.md);
  генератор и проверка ссылок: `tools/connection_link.py`.
- **QR-код соединения.** В меню сохранённого соединения (Интернет/TCP, Радио) пункт
  «Поделиться QR-кодом» показывает QR со ссылкой, умеет копировать ссылку и
  отправлять картинку. Если в ссылке есть парольная фраза IFAC, приложение предупреждает,
  что она лежит в коде открытым текстом.
- **Экспорт и импорт контактов и подключений.** **Настройки → Идентичность →
  Экспорт/Импорт контактов и подключений** сохраняет контакты и все подключения
  Интернет/TCP в JSON-файл и добавляет их из файла на другом устройстве. Парольные
  фразы IFAC попадают в файл только по явному согласию, файл не шифруется. Существующие
  контакты и подключения не перезаписываются.
- **Радар распространения анонсов.** **Сеть → Активность → Распространение анонсов**:
  ваш узел в центре, кольца — хопы, сектора — интерфейсы, точки — известные получатели.
  Новый анонс расходится волной от своей точки, а импульс идёт к центру. Данные берутся
  из таблицы путей и событий анонсов, которые приложение уже получает.

Список изменений по версиям: [CHANGELOG.md](CHANGELOG.md).

## What It Is

Ratspeak is for private messaging when the normal internet is unavailable,
untrusted, or not the path you want to depend on. When your cell tower is down, when natural disaster hits, or when you just want an alternative. When you know the current system is broken.

It runs on
[Reticulum](https://github.com/ratspeak/rsReticulum) and [LXMF](https://github.com/ratspeak/rsLXMF), so conversations can happen
over regular internet, LoRa radios, WiFi, Bluetooth, there is no limit - if it can move data it can be a part of the mesh.

There is no Ratspeak account server, no central database, no hub where everything routes through by default. Your Reticulum identity is generated on
your device and becomes your address on the mesh, no personal information needed.

## Current State

Ratspeak is in experimental/alpha status. That means there are bugs, there are quirks, and things are not perfect. If something isn't working up to your standards, or at all, open an issue with the details needed to reproduce it. Useful, direct feedback helps us fix things faster.

Supported app targets are macOS, Windows, Linux, Android, and iOS. Public
desktop and Android packages will be linked from
[ratspeak.org/download.html](https://ratspeak.org/download.html) as they are
released. iOS does not have a public download yet; macOS is unsigned, and the
Windows MSIX still needs signing for BLE Peering support. These distribution
lanes are in progress.

## What You Get

- Account-free messaging over Reticulum.
- Full offline messaging support.
- Shared Channels, including local history, member presence, and optional hub
  hosting.
- Local Network, TCP, RNode/LoRa support, Bluetooth Peering, and more.
- Contacts, discovered peers, path requests, interface status, propagation
  status, and transport health in the app.
- Activity tools for understanding network and messaging events without
  leaving the app.
- Experimental peer-to-peer voice calls over [LXST](https://github.com/ratspeak/rsLXST)
  (contacts-only, 0-hop, native microphone/speaker).
- Experimental voice messages with local recording and playback.
- Chess, Tic-Tac-Toe, and Four in a Row.
- Built-in light/dark modes, selectable color themes, and adjustable text size.

## Install

Use the download page when public builds are available:
[ratspeak.org/download.html](https://ratspeak.org/download.html).

For setup help, see:

- [Install and Platform Setup](https://docs.ratspeak.org/docs/getting-started/install-and-platform-setup)
- [Your First Session](https://docs.ratspeak.org/docs/getting-started/your-first-session)
- [Troubleshooting](https://docs.ratspeak.org/docs/reference/troubleshooting)

## Build From Source

Advanced local-stack sharing in this source tree is described in
[Local Reticulum ownership](docs/network-ownership.md).

Форк собирается так же, как оригинал. The full build guide is here:
[Building from Source](https://docs.ratspeak.org/docs/reference/building-from-source).
It covers desktop prerequisites, Android APKs, iOS signing, and the required
sibling checkout layout.

After installing the desktop prerequisites, the shortest local path is:

```bash
mkdir ratspeak-src
cd ratspeak-src
git clone https://github.com/ratspeak/rsReticulum
git clone https://github.com/ratspeak/rsLXMF
git clone https://github.com/ratspeak/lrgp-rs
git clone https://github.com/ratspeak/rsLXST   # experimental voice; skip with --no-default-features
git clone https://github.com/CleoWixom/Ratspeak-ru Ratspeak   # форк; оригинал: https://github.com/ratspeak/Ratspeak

cd Ratspeak
bash dashboard/build-css.sh
cd src-tauri
cargo tauri dev
```

Соседние репозитории (`rsReticulum`, `rsLXMF`, `lrgp-rs`, `rsLXST`) берутся из оригинала: форк их не менял.
Теги ниже относятся к релизам оригинала; сборка форка из `main` использует
зафиксированные коммиты из `release/dependency-set.json`.

To reproduce a tagged Ratspeak release starting with `v1.0.29`, check out the
Ratspeak `vX.Y.Z` tag and the matching annotated `ratspeak-vX.Y.Z` tag in each
of `rsReticulum`, `rsLXMF`, `rsLXST`, and `lrgp-rs`. The exact commits in
`Ratspeak/release/dependency-set.json` are authoritative; the coordinated tags
are permanent readable aliases for that graph. From the sibling layout above,
`node Ratspeak/scripts/release/source-integrity.mjs verify-tags` verifies every
component version, annotated tag, and exact commit before a build.

For an automated exact checkout, use
`node scripts/release/checkout-release-source.mjs --tag vX.Y.Z --destination ../release-source`
from a tools checkout containing the helper. See
[Source packaging](docs/source-packaging.md) for plan mode, archive handling, and
Nix packaging. Fetch component `commit` values from the selected release's
manifest; component `version` values must not be used as standalone source tags.

For a release bundle, run `cargo tauri build -- --locked` from
`Ratspeak/src-tauri`.
Desktop bundles land under `Ratspeak/src-tauri/target/release/bundle/`.

To build without the experimental voice stack and skip the rsLXST sibling,
pass `--no-default-features` to `cargo tauri dev` or `cargo tauri build`.

## Voice (experimental)

Voice calls run on [LXST](https://github.com/ratspeak/rsLXST) over Reticulum
links — no servers, no relays. The stack is new and intentionally narrow:

- Microphone and speaker access goes through the OS: `RECORD_AUDIO` and
  `MODIFY_AUDIO_SETTINGS` on Android, `NSMicrophoneUsageDescription` on
  macOS/iOS, the default audio device on Linux/Windows.
- Incoming calls are restricted to contacts on a cached 0-hop path. Calls
  from non-contacts are dropped before any audio path opens.
- Persistently rejected callers are blackholed (rate-limit reason) so they
  cannot keep ringing the device.
- The `lxst-voice` Cargo feature is on by default. Disable it with
  `cargo tauri dev --no-default-features` if you want to build Ratspeak
  without the voice stack.

Voice is experimental — expect rough edges. Codec quality, call setup,
ringtones, and platform audio routing are all subject to change.

## Platform Notes

- iOS does not support general USB serial. Ratspeak's signed iOS builds support
  Local Network and multicast discovery; users must still grant the normal iOS
  Local Network permission. Notifications require user permission, and
  background execution remains subject to iOS lifecycle limits.
- Windows Bluetooth Peer advertiser support needs the future signed MSIX lane.
- Linux Bluetooth Peer depends on BlueZ GATT server and LE advertising support.
- Voice calls require microphone permission per platform; the prompt is
  triggered the first time you place or answer a call.

## License

GNU Affero General Public License v3.0 or later. See [`LICENSE`](LICENSE).
