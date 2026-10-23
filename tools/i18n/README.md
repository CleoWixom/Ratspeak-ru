# Локализация Ratspeak (русификация и другие языки)

Интерфейс дашборда содержит жёстко заданные английские строки, поэтому перевод делается
рантайм-слоем без правки логики приложения.

| Файл | Назначение |
|---|---|
| `tools/i18n/extract.py [--lang xx]` | Находит UI-строки в `dashboard/*.html`, `static/js/*.js`, `src-tauri/loading/index.html`, пишет `locales/en.json` и дополняет `locales/<lang>.json` (новые ключи — с пустым значением) |
| `tools/i18n/merge.py <lang> <batch.json>` | Вливает пакет переводов `{ "English": "Перевод" }` |
| `tools/i18n/build.py <lang>` | Собирает `locales/<lang>.json` → `locales/<lang>.js` (`window.RATSPEAK_I18N`) |
| `dashboard/static/js/i18n.js` | Рантайм: переводит текстовые узлы и атрибуты `placeholder/title/aria-label/data-tooltip/alt`, следит за DOM через MutationObserver |

Язык берётся из `localStorage['ratspeak_lang']` (например `'ru'`, `'en'` — отключает перевод)
или из языка системы.

## Новый язык
1. `python3 tools/i18n/extract.py --lang de`
2. Заполнить значения в `dashboard/static/locales/de.json`
3. `python3 tools/i18n/build.py de`
4. Подключить `locales/de.js` в `dashboard/index.html` вместо/рядом с `ru.js`.

## Известные ограничения
- Строки, собираемые конкатенацией в JS (`'Latitude must be between ' + a + ...`), и динамические
  счётчики не переводятся точным совпадением — нужен шаблонный проход (TODO).
- Длинные юридические тексты (`legal_documents.js`) и эмодзи-данные пропускаются.
- Статус: ru — 1105 из 1138 ключей; оставшиеся — фрагменты конкатенации.
