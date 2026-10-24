# Локализация Ratspeak (русификация и другие языки)

Интерфейс дашборда содержит жёстко заданные английские строки, поэтому перевод делается
рантайм-слоем без правки логики приложения.

| Файл | Назначение |
|---|---|
| `tools/i18n/extract.py [--lang xx]` | Находит UI-строки в `dashboard/*.html`, `static/js/*.js`, `src-tauri/loading/index.html`, пишет `locales/en.json` и дополняет `locales/<lang>.json` (новые ключи — с пустым значением) |
| `tools/i18n/merge.py <lang> <batch.json>` | Вливает пакет переводов `{ "English": "Перевод" }` |
| `tools/i18n/extract_patterns.py [--lang xx]` | Находит динамические строки (`'a ' + x`, `` `a ${x}` ``) и записывает шаблоны с `{0}`, `{1}` в `locales/<lang>.patterns.json` |
| `tools/i18n/merge_patterns.py <lang> <batch.json>` | Вливает переводы шаблонов (точный ключ или однозначное начало) |
| `tools/i18n/build.py <lang>` | Собирает `locales/<lang>.json` и `.patterns.json` → `locales/<lang>.js` (`RATSPEAK_I18N` и `RATSPEAK_I18N_PATTERNS`) |
| `dashboard/static/js/i18n.js` | Рантайм: переводит текстовые узлы и атрибуты `placeholder/title/aria-label/data-tooltip/alt`, следит за DOM через MutationObserver |

Язык берётся из `localStorage['ratspeak_lang']` (например `'ru'`, `'en'` — отключает перевод)
или из языка системы.

## Новый язык
1. `python3 tools/i18n/extract.py --lang de`
2. Заполнить значения в `dashboard/static/locales/de.json`
3. `python3 tools/i18n/build.py de`
4. Подключить `locales/de.js` в `dashboard/index.html` вместо/рядом с `ru.js`.

## Шаблоны и плюралы
В значении шаблона `{N}` подставляет N-ю захваченную часть; `{N:запись|записи|записей}` выбирает
русскую форму по числу из `{N}` (1 / 2–4 / 5+). Захваченный текст, который сам есть в словаре
(например `Remove`, `Grant`), переводится автоматически.

## Известные ограничения
- Строки, которые разрезаны тегами посреди предложения (`<strong>12</strong>-word`), переводятся кусками; порядок слов должен совпадать.
- Длинные юридические тексты (`legal_documents.js`) и эмодзи-данные пропускаются.
- Статус: ru — 2513 из 2785 точных ключей (остальные — шум: CSS-селекторы, логи, темы оформления и обрывки склеенных строк, закрытые шаблонами) и 114 шаблонов.
