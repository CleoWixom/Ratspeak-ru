# Ссылки подключения (`ratspeak://tcp`, `ratspeak://radio`)

Ссылка открывает в приложении форму **Сеть → Соединения → Интернет/TCP** или **Радио**
с заполненными полями. Приложение ничего не подключает само: пользователь проверяет
данные и нажимает «Подключить».

## Интернет/TCP

```
ratspeak://tcp?v=1&host=rns.example.org&port=4242&name=Мой%20узел&ifac_name=mynet&ifac_pass=secret
```

| Поле | Обязательно | Описание |
|---|---|---|
| `v` | да | версия формата, `1` |
| `host` | да | имя хоста, IPv4 или IPv6 |
| `port` | нет | 1–65535, по умолчанию 4242 |
| `name` | нет | имя соединения (до 64 байт, без `[ ] = #`) |
| `ifac_name` | нет | имя сети IFAC |
| `ifac_pass` | нет | парольная фраза IFAC |

IFAC включается, если задано хотя бы одно из `ifac_name` / `ifac_pass`.
**Парольная фраза хранится в ссылке открытым текстом** — делитесь такими ссылками
только с доверенными людьми. Значения не могут содержать `#` и управляющие символы
(они записываются в конфиг Reticulum).

## Радио (LoRa)

Ссылка содержит только радиопараметры. Устройство (BLE, USB, serial, TCP) выбирается
пользователем на своём телефоне или компьютере.

Вариант 1, регион и пресет: `region` (`americas`, `europe`, `uhf_433`, `australia`, `asia`)
и `preset` (`medium_fast`, `short_turbo`, `short_fast`, `short_slow`, `medium_slow`,
`long_turbo`, `long_fast`, `long_moderate`).

Вариант 2, свои параметры, все пять сразу: `freq` (Гц), `bw` (Гц), `sf` (5–12),
`cr` (5–8), `tx` (0–37 дБм).

Нельзя смешивать варианты. Дополнительно: `name`, `mode` (`full`, `gateway`,
`access_point`, `boundary`, `roaming`), `airtime_short`, `airtime_long` (проценты, >0 и ≤100).

```
ratspeak://radio?v=1&region=europe&preset=medium_fast&name=Город&mode=gateway
ratspeak://radio?v=1&freq=868000000&bw=250000&sf=9&cr=5&tx=14
```

## Инструменты и проверка

```sh
python3 tools/connection_link.py tcp --host rns.example.org --ifac-name mynet --ifac-pass secret
python3 tools/connection_link.py radio --region europe --preset medium_fast
python3 tools/connection_link.py check "ratspeak://tcp?v=1&host=rns.example.org"
```

Единственный разборщик ссылок в приложении — `crates/ratspeak-tauri/src/commands/connection_link.rs`;
`tools/connection_link.py` повторяет его правила. Тесты: `cargo test -p ratspeak-tauri connection_link`,
`node scripts/dashboard/test_connection_link.js`.

## QR-код соединения

В меню сохранённого соединения (Интернет/TCP или Радио) есть пункт **«Поделиться QR-кодом»**.
Он показывает QR с ссылкой `ratspeak://tcp` или `ratspeak://radio` и позволяет скопировать ссылку
или отправить картинку. Получатель сканирует код камерой телефона: ссылка откроет форму
добавления соединения с уже заполненными полями.

- TCP: хост, порт, имя и, если включён IFAC, имя сети и парольная фраза. Парольная фраза
  попадает в QR открытым текстом, приложение предупреждает об этом.
- Радио: точные частота, полоса, SF, CR, мощность, режим и лимиты эфира. Радиоустройство
  (BLE, USB, serial) в код не входит.
- Ссылка проверяется тем же Rust-парсером (`validate_connection_link`), что и входящие.
  Формат QR вмещает до 241 байта: если имя не помещается, оно опускается, а если не помещаются
  сами данные, показывается ошибка.

## `lxma://` addresses

`lxma://<32 hex destination hash>` and `lxma://<32 hex destination hash>.<128 hex public key>`
open the **Add contact** dialog with the hash filled in; nothing is added until the user confirms.
The parser (`parse_lxma` in `connection_link.rs`, mirrored by `RuLxma.parse` in `ru_lxma.js`) accepts
only that exact shape: hex of the right length (any case, stored lowercase), an optional single
trailing `/`, no query, path, userinfo or whitespace.

Delivery uses the same inbox as `ratspeak://tcp` / `ratspeak://radio`: the Android manifest filter
and deep-link runtime filter (scheme `lxma`, no host), the desktop `schemes` list, the Android share
sheet (a shared text that is exactly one link), and links tapped or pasted inside the app.
The public key is validated and carried in the target, but the backend `add_contact` command
only takes the hash today, so the key is not stored yet.
