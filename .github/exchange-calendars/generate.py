#!/usr/bin/env python3
"""Build the exchange calendar snapshot the app ships and syncs weekly.

Reads the regular hours, lunch breaks, holidays, early closes and late opens of each exchange
below from `exchange_calendars` (https://github.com/gerrymanoim/exchange_calendars) and writes
them as one JSON file. The app embeds this file at build time and, once a week, fetches the
`calendars.json` asset of the `exchange-calendars` release, which
`.github/workflows/exchange-calendars.yml` regenerates every Monday. Running it by hand is only
needed to refresh the embedded copy before a release.

Usage:
    python3 -m venv .venv && .venv/bin/pip install exchange_calendars
    .venv/bin/python .github/exchange-calendars/generate.py \
        > core/otw-core/src/exchanges/calendars.json

Shape (minutes are local wall clock in the exchange's timezone):
    {generated, source, from, to, exchanges: [{mic, code, name, city, timezone, open, close,
     break_start, break_end, prior_day_open, weekdays, closed: [date], special: [{date, open,
     close}], covered_to}]}

A session is labelled by its trading date. `prior_day_open` = the session opens the evening
before its label (CME Globex). `weekdays` masks the labels, Monday = bit 0. `closed` lists the
labels that fall on a weekday but do not trade; `special` the ones whose open or close differs
from the regular hours. `covered_to` is the last date the holidays are known for: some venues
publish one year at a time.
"""

import datetime as dt
import json
import sys
from collections import Counter

import exchange_calendars as xc

# (calendar, MIC shown, trigram, name, city). The trigram is the city's, like an airport code,
# except where two venues share a city.
EXCHANGES = [
    ("XASX", "XASX", "SYD", "Australian Securities Exchange", "Sydney"),
    ("XNZE", "XNZE", "WLG", "NZX", "Wellington"),
    ("XTKS", "XTKS", "TYO", "Tokyo Stock Exchange", "Tokyo"),
    ("XKRX", "XKRX", "SEL", "Korea Exchange", "Seoul"),
    ("XSHG", "XSHG", "SHA", "Shanghai Stock Exchange", "Shanghai"),
    ("XHKG", "XHKG", "HKG", "Hong Kong Exchanges", "Hong Kong"),
    ("XTAI", "XTAI", "TPE", "Taiwan Stock Exchange", "Taipei"),
    ("XSES", "XSES", "SIN", "Singapore Exchange", "Singapore"),
    ("XBOM", "XBOM", "BOM", "BSE", "Mumbai"),
    ("XJSE", "XJSE", "JNB", "Johannesburg Stock Exchange", "Johannesburg"),
    ("XETR", "XETR", "FRA", "Xetra", "Frankfurt"),
    ("XPAR", "XPAR", "PAR", "Euronext Paris", "Paris"),
    ("XAMS", "XAMS", "AMS", "Euronext Amsterdam", "Amsterdam"),
    ("XLON", "XLON", "LON", "London Stock Exchange", "London"),
    ("XSWX", "XSWX", "ZRH", "SIX Swiss Exchange", "Zurich"),
    ("XMIL", "XMIL", "MIL", "Borsa Italiana", "Milan"),
    ("XMAD", "XMAD", "MAD", "Bolsa de Madrid", "Madrid"),
    ("XSTO", "XSTO", "STO", "Nasdaq Stockholm", "Stockholm"),
    ("XNYS", "XNYS", "NYS", "New York Stock Exchange", "New York"),
    ("XNYS", "XNAS", "NAS", "Nasdaq", "New York"),
    ("CMES", "XCME", "CHI", "CME Globex (equity futures)", "Chicago"),
    ("XTSE", "XTSE", "TOR", "Toronto Stock Exchange", "Toronto"),
    ("BVMF", "BVMF", "SAO", "B3", "Sao Paulo"),
    ("XMEX", "XMEX", "MEX", "Bolsa Mexicana de Valores", "Mexico City"),
]

# Where exchange_calendars models a venue differently from how it trades. CME Globex equity
# futures halt at 16:00 Chicago for the daily maintenance hour; the library runs the session
# to 17:00, the next open.
CLOSE_REMAP = {"XCME": {1020: 960}}


def minute(ts, tz):
    local = ts.tz_convert(tz)
    return local.hour * 60 + local.minute


def calendar(cal_name, start, end):
    """The calendar over [start, end], or up to the last year its holidays are recorded for
    (some venues publish one year at a time). The returned `end` is how far `closed` is true."""
    last = dt.date.fromisoformat(end)
    while True:
        try:
            return xc.get_calendar(cal_name, start=start, end=last.isoformat()), last.isoformat()
        except ValueError:
            if last.year <= dt.date.today().year:
                raise
            last = dt.date(last.year - 1, 12, 31)


def build(cal_name, mic, start, end):
    cal, end = calendar(cal_name, start, end)
    tz = str(cal.tz)
    sched = cal.schedule.loc[start:end]
    weekmask = cal.weekmask  # e.g. "1111100", Monday first
    weekdays = sum(1 << i for i, c in enumerate(weekmask) if c == "1")

    opens = [minute(o, tz) for o in sched["open"]]
    remap = CLOSE_REMAP.get(mic, {})
    closes = [remap.get(m, m) for m in (minute(c, tz) for c in sched["close"])]
    reg_open = Counter(opens).most_common(1)[0][0]
    reg_close = Counter(closes).most_common(1)[0][0]
    prior = [o.tz_convert(tz).date() < d.date() for d, o in zip(sched.index, sched["open"])]
    prior_day_open = Counter(prior).most_common(1)[0][0]

    bs = be = None
    if "break_start" in sched and sched["break_start"].notna().any():
        bs = Counter(minute(x, tz) for x in sched["break_start"].dropna()).most_common(1)[0][0]
        be = Counter(minute(x, tz) for x in sched["break_end"].dropna()).most_common(1)[0][0]

    sessions = {d.date() for d in sched.index}
    closed = []
    day = dt.date.fromisoformat(start)
    last = dt.date.fromisoformat(end)
    while day <= last:
        if weekmask[day.weekday()] == "1" and day not in sessions:
            closed.append(day.isoformat())
        day += dt.timedelta(days=1)

    special = []
    for d, o, c in zip(sched.index, opens, closes):
        if o != reg_open or c != reg_close:
            special.append(
                {"date": d.date().isoformat(), "open": o if o != reg_open else None,
                 "close": c if c != reg_close else None}
            )

    return {
        "timezone": tz,
        "open": reg_open,
        "close": reg_close,
        "break_start": bs,
        "break_end": be,
        "prior_day_open": bool(prior_day_open),
        "weekdays": weekdays,
        "closed": closed,
        "special": special,
        "covered_to": end,
    }


def main():
    today = dt.date.today()
    start = dt.date(today.year - 1, 1, 1).isoformat()
    end = dt.date(today.year + 2, 12, 31).isoformat()
    out = []
    for cal_name, mic, code, name, city in EXCHANGES:
        row = {"mic": mic, "code": code, "name": name, "city": city}
        row.update(build(cal_name, mic, start, end))
        out.append(row)
    json.dump(
        {
            "generated": dt.datetime.now(dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
            "source": f"exchange_calendars {xc.__version__}",
            "from": start,
            "to": end,
            "exchanges": out,
        },
        sys.stdout,
        indent=1,
        ensure_ascii=False,
    )
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
