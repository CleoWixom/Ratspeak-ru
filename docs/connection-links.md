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
