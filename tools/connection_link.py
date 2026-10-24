#!/usr/bin/env python3
"""Генератор и проверка ссылок ratspeak://tcp и ratspeak://radio.

Ссылка открывает в приложении обычную форму «Сеть → Соединения → Интернет/TCP»
или «Радио» с уже заполненными полями. Приложение само ничего не подключает:
пользователь проверяет данные и нажимает «Подключить».

Примеры:
  tools/connection_link.py tcp --host rns.example.org --port 4242 --name "Мой узел"
  tools/connection_link.py tcp --host 10.0.0.5 --ifac-name mynet --ifac-pass secret
  tools/connection_link.py radio --region europe --preset medium_fast --name "Город"
  tools/connection_link.py radio --freq 868000000 --bw 250000 --sf 9 --cr 5 --tx 14
  tools/connection_link.py check "ratspeak://tcp?v=1&host=rns.example.org"
  tools/connection_link.py selftest

Правила совпадают с crates/ratspeak-tauri/src/commands/connection_link.rs.
Парольная фраза IFAC лежит в ссылке открытым текстом: передавайте такие ссылки
только доверенным людям.
"""
import argparse
import ipaddress
import re
import sys
from urllib.parse import parse_qsl, quote, urlsplit

MAX_LINK_BYTES = 512
NAME_MAX = 64
IFAC_MAX = 128
MODES = ("full", "gateway", "access_point", "boundary", "roaming")
REGIONS = ("americas", "europe", "uhf_433", "australia", "asia")
PRESETS = ("medium_fast", "short_turbo", "short_fast", "short_slow",
           "medium_slow", "long_turbo", "long_fast", "long_moderate")
LIMITS = {"freq": (137_000_000, 3_000_000_000), "bw": (7_800, 1_625_000),
          "sf": (5, 12), "cr": (5, 8), "tx": (0, 37)}
TCP_FIELDS = {"v", "host", "port", "name", "ifac_name", "ifac_pass"}
RADIO_FIELDS = {"v", "name", "mode", "region", "preset", "freq", "bw", "sf", "cr", "tx",
                "airtime_short", "airtime_long"}


class LinkError(ValueError):
    pass


def _uint(value, field, lo, hi):
    if not re.fullmatch(r"(0|[1-9][0-9]{0,11})", value or ""):
        raise LinkError(f"поле {field}: не число")
    number = int(value)
    if not lo <= number <= hi:
        raise LinkError(f"поле {field}: вне диапазона {lo}..{hi}")
    return number


def _host(host):
    if not host or len(host.encode()) > 253:
        return False
    if ":" in host:
        try:
            ipaddress.IPv6Address(host)
            return True
        except ValueError:
            return False
    return all(label and len(label) <= 63 and not label.startswith("-") and
               not label.endswith("-") and re.fullmatch(r"[A-Za-z0-9_-]+", label)
               for label in host.split("."))


def _text(value, limit, forbidden):
    return (bool(value) and len(value.encode()) <= limit and value == value.strip() and
            not any(ord(c) < 32 or ord(c) == 127 or c in forbidden for c in value))


def _name(value):
    return _text(value, NAME_MAX, "[]=#")


def _ifac(value):
    return _text(value, IFAC_MAX, "#")


def _percent(value, field):
    if not re.fullmatch(r"(?!\.)[0-9]+(\.[0-9]+)?", value or "") or len(value) > 8:
        raise LinkError(f"поле {field}: не процент")
    number = float(value)
    if not 0 < number <= 100:
        raise LinkError(f"поле {field}: нужно больше 0 и не больше 100")
    return number


def parse(link):
    """Проверяет ссылку так же, как приложение. Возвращает (вид, поля) или LinkError."""
    if link != link.strip() or not link or len(link.encode()) > MAX_LINK_BYTES:
        raise LinkError("ссылка пуста, слишком длинная или содержит пробелы по краям")
    parts = urlsplit(link)
    if (parts.scheme != "ratspeak" or parts.fragment or parts.username or
            parts.password or parts.path not in ("", "/")):
        raise LinkError("это не ссылка ratspeak://tcp или ratspeak://radio")
    try:
        if parts.port is not None:
            raise LinkError("порт в адресе ссылки не допускается")
    except ValueError:
        raise LinkError("некорректный адрес ссылки")
    kind = parts.hostname
    if kind not in ("tcp", "radio"):
        raise LinkError("поддерживаются только ratspeak://tcp и ratspeak://radio")
    allowed = TCP_FIELDS if kind == "tcp" else RADIO_FIELDS
    fields = {}
    for key, value in parse_qsl(parts.query, keep_blank_values=True):
        if key not in allowed:
            raise LinkError(f"неподдерживаемое поле {key}")
        if key in fields:
            raise LinkError(f"поле {key} повторяется")
        fields[key] = value
    if fields.pop("v", None) != "1":
        raise LinkError("неподдерживаемая версия ссылки (нужно v=1)")
    (_check_tcp if kind == "tcp" else _check_radio)(fields)
    return kind, fields


def _check_tcp(f):
    if not _host(f.get("host", "")):
        raise LinkError("нужен корректный host")
    if "port" in f:
        _uint(f["port"], "port", 1, 65535)
    if "name" in f and not _name(f["name"]):
        raise LinkError("некорректное имя")
    for key in ("ifac_name", "ifac_pass"):
        if key in f and not _ifac(f[key]):
            raise LinkError(f"некорректное значение {key}")


def _check_radio(f):
    if "name" in f and not _name(f["name"]):
        raise LinkError("некорректное имя")
    if "mode" in f and f["mode"] not in MODES:
        raise LinkError("неизвестный режим интерфейса")
    named = "region" in f or "preset" in f
    custom = any(k in f for k in LIMITS)
    if named == custom:
        raise LinkError("нужно либо region и preset, либо freq, bw, sf, cr и tx")
    if named:
        if f.get("region") not in REGIONS or f.get("preset") not in PRESETS:
            raise LinkError("неизвестный регион или пресет")
    else:
        for key, (lo, hi) in LIMITS.items():
            if key not in f:
                raise LinkError("для своих параметров нужны freq, bw, sf, cr и tx")
            _uint(f[key], key, lo, hi)
    for key in ("airtime_short", "airtime_long"):
        if key in f:
            _percent(f[key], key)


def build(kind, fields):
    order = (["host", "port", "name", "ifac_name", "ifac_pass"] if kind == "tcp" else
             ["name", "mode", "region", "preset", "freq", "bw", "sf", "cr", "tx",
              "airtime_short", "airtime_long"])
    pairs = [("v", "1")] + [(k, str(fields[k])) for k in order if fields.get(k) not in (None, "")]
    link = f"ratspeak://{kind}?" + "&".join(f"{k}={quote(v, safe='')}" for k, v in pairs)
    parse(link)  # то, что выдаём, приложение всегда примет
    return link


def selftest():
    good = [
        "ratspeak://tcp?v=1&host=rns.example.org",
        "ratspeak://tcp?v=1&host=10.0.0.5&port=7000&name=My%20node&ifac_name=mynet&ifac_pass=p%40ss",
        "ratspeak://tcp?v=1&host=2001%3Adb8%3A%3A1",
        "ratspeak://radio?v=1&region=europe&preset=medium_fast&mode=gateway",
        "ratspeak://radio?v=1&freq=868000000&bw=250000&sf=9&cr=5&tx=14&airtime_long=10",
    ]
    bad = [
        "", " ratspeak://tcp?v=1&host=a.example", "ratspeak://tcp?host=a.example",
        "ratspeak://tcp?v=1", "ratspeak://tcp?v=1&host=a.example.",
        "ratspeak://tcp?v=1&host=a.example&port=0", "ratspeak://tcp?v=1&host=a.example&port=080",
        "ratspeak://tcp?v=1&host=a.example&ifac_pass=a%23b",
        "ratspeak://tcp?v=1&host=a.example&name=%5B%5Bx%5D%5D",
        "ratspeak://tcp?v=1&host=a.example&extra=1",
        "ratspeak://tcp?v=1&v=1&host=a.example", "ratspeak://tcp:80?v=1&host=a.example",
        "ratspeak://channel?v=1&hub=00112233445566778899aabbccddeeff",
        "ratspeak://radio?v=1", "ratspeak://radio?v=1&region=europe",
        "ratspeak://radio?v=1&region=europe&preset=medium_fast&freq=868000000",
        "ratspeak://radio?v=1&freq=868000000&bw=250000&sf=13&cr=5&tx=14",
        "ratspeak://radio?v=1&region=europe&preset=medium_fast&mode=turbo",
        "ratspeak://radio?v=1&region=europe&preset=medium_fast&airtime_long=0",
        "ratspeak://radio?v=1&region=europe&preset=medium_fast&port=ble%3A%2F%2Faa",
    ]
    for link in good:
        parse(link)
    for link in bad:
        try:
            parse(link)
        except LinkError:
            continue
        raise AssertionError(f"принята некорректная ссылка: {link}")
    link = build("tcp", {"host": "node.example", "port": 4242, "name": "Узел",
                         "ifac_name": "сеть", "ifac_pass": "пароль"})
    assert parse(link)[1]["ifac_pass"] == "пароль"
    print("selftest: ок")


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    t = sub.add_parser("tcp", help="ссылка для Интернет/TCP")
    t.add_argument("--host", required=True)
    t.add_argument("--port", type=int, default=None, help="по умолчанию 4242")
    t.add_argument("--name")
    t.add_argument("--ifac-name", dest="ifac_name", help="имя сети IFAC (необязательно)")
    t.add_argument("--ifac-pass", dest="ifac_pass", help="парольная фраза IFAC (необязательно)")
    r = sub.add_parser("radio", help="ссылка с параметрами радио (LoRa)")
    r.add_argument("--name")
    r.add_argument("--mode", choices=MODES)
    r.add_argument("--region", choices=REGIONS)
    r.add_argument("--preset", choices=PRESETS)
    for key in ("freq", "bw", "sf", "cr", "tx"):
        r.add_argument(f"--{key}", type=int)
    r.add_argument("--airtime-short", dest="airtime_short")
    r.add_argument("--airtime-long", dest="airtime_long")
    c = sub.add_parser("check", help="проверить ссылку")
    c.add_argument("link")
    sub.add_parser("selftest", help="самопроверка правил")
    a = ap.parse_args(argv)
    try:
        if a.cmd == "selftest":
            selftest()
        elif a.cmd == "check":
            kind, fields = parse(a.link)
            shown = {k: ("***" if k == "ifac_pass" else v) for k, v in fields.items()}
            print(f"ок: {kind} {shown}")
        else:
            fields = {k: v for k, v in vars(a).items() if k != "cmd" and v is not None}
            print(build(a.cmd, fields))
    except LinkError as error:
        print(f"ошибка: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
