// format.js is the one place numbers, money, percentages and dates become text. Every
// expected string below is written by hand from the rules in its header (U+2212 minus,
// U+2014 for a missing value, sign before the symbol, ISO minor units), not produced by
// the code under test.
import { afterEach, describe, expect, test, vi } from 'vitest';
import { get } from 'svelte/store';
import { locale } from '$lib/i18n';
import {
  EM_DASH,
  fmtMoney,
  fmtSignedMoney,
  fmtCompactMoney,
  fmtCompactValue,
  fmtPct,
  fmtRatioPct,
  fmtSignedPct,
  fmtNum,
  fmtFixed,
  fmtBytes,
  fmtDate,
  fmtDateTime,
  dateKey,
  fmtDuration,
  relativeTime,
  shortAge,
  ago
} from '$lib/format.js';
import { withNumberLocale } from '../test/locale.js';

const MINUS = '\u2212';
const NBSP = '\u00a0';
// fr-FR groups thousands with U+202F NARROW NO-BREAK SPACE.
const NNBSP = '\u202f';
const en = (fn) => withNumberLocale('en-US', fn);
const fr = (fn) => withNumberLocale('fr-FR', fn);

describe('missing values', () => {
  test('null, undefined and NaN all render the one em dash', () => {
    expect(EM_DASH).toBe('\u2014');
    for (const v of [null, undefined, NaN]) {
      expect(fmtMoney(v)).toBe(EM_DASH);
      expect(fmtSignedMoney(v)).toBe(EM_DASH);
      expect(fmtPct(v)).toBe(EM_DASH);
      expect(fmtRatioPct(v)).toBe(EM_DASH);
      expect(fmtSignedPct(v)).toBe(EM_DASH);
      expect(fmtNum(v)).toBe(EM_DASH);
      expect(fmtFixed(v)).toBe(EM_DASH);
      expect(fmtBytes(v)).toBe(EM_DASH);
      expect(fmtCompactValue(v)).toBe(EM_DASH);
    }
    expect(fmtCompactMoney(null, 'n/a')).toBe('n/a');
  });
});

describe('money, en-US', () => {
  test('a positive amount carries the symbol and two decimals', () => {
    en(() => expect(fmtMoney(1234.5, 'USD')).toBe('$1,234.50'));
  });

  test('a negative amount puts a true minus before the symbol', () => {
    en(() => expect(fmtMoney(-1234.5, 'USD')).toBe(`${MINUS}$1,234.50`));
  });

  test('zero is a value, not a missing one', () => {
    en(() => {
      expect(fmtMoney(0, 'USD')).toBe('$0.00');
      expect(fmtSignedMoney(0, 'USD')).toBe('+$0.00');
    });
  });

  test('each currency keeps its own minor unit and symbol', () => {
    en(() => {
      expect(fmtMoney(1234.5, 'JPY')).toBe('¥1,235');
      expect(fmtMoney(1234.5, 'EUR')).toBe('€1,234.50');
      expect(fmtMoney(1234.5, 'GBP')).toBe('£1,234.50');
      expect(fmtMoney(1234.5, 'CNY')).toBe('CN¥1,234.50');
      expect(fmtMoney(1234.5, 'CHF')).toBe(`CHF${NBSP}1,234.50`);
      // Outside the symbol table the ISO code is printed, never dropped.
      expect(fmtMoney(1234.5, 'SEK')).toBe(`SEK${NBSP}1,234.50`);
      // BHD has three minor digits.
      expect(fmtMoney(1.5, 'BHD')).toBe(`BHD${NBSP}1.500`);
    });
  });

  test('an explicit digits argument overrides the minor unit', () => {
    en(() => expect(fmtMoney(1234.5, 'USD', 4)).toBe('$1,234.5000'));
  });

  test('very large amounts keep every digit and their grouping', () => {
    en(() => {
      expect(fmtMoney(1e15, 'USD')).toBe('$1,000,000,000,000,000.00');
      expect(fmtMoney(-987654321012.34, 'EUR')).toBe(`${MINUS}€987,654,321,012.34`);
    });
  });

  test('signed money always prints its sign, before the symbol', () => {
    en(() => {
      expect(fmtSignedMoney(98, 'USD')).toBe('+$98.00');
      expect(fmtSignedMoney(-52, 'USD')).toBe(`${MINUS}$52.00`);
      expect(fmtSignedMoney(-1234.5, 'SEK')).toBe(`${MINUS}SEK${NBSP}1,234.50`);
    });
  });

  test('a half cent rounds away from zero, like the decimal the user typed', () => {
    en(() => {
      expect(fmtMoney(1.005, 'USD')).toBe('$1.01');
      expect(fmtMoney(2.675, 'USD')).toBe('$2.68');
      expect(fmtMoney(-2.675, 'USD')).toBe(`${MINUS}$2.68`);
    });
  });
});

describe('money, fr-FR', () => {
  test('separators follow the browser locale, the symbol still leads', () => {
    fr(() => {
      expect(fmtMoney(1234.5, 'EUR')).toBe(`€1${NNBSP}234,50`);
      expect(fmtMoney(-1234.5, 'USD')).toBe(`${MINUS}$1${NNBSP}234,50`);
      expect(fmtSignedMoney(1234.5, 'EUR')).toBe(`+€1${NNBSP}234,50`);
    });
  });

  test('a currency outside the table is laid out by Intl, code after the amount', () => {
    fr(() => {
      expect(fmtMoney(1234.5, 'SEK')).toBe(`1${NNBSP}234,50${NBSP}SEK`);
      expect(fmtSignedMoney(-1234.5, 'SEK')).toBe(`${MINUS}1${NNBSP}234,50${NBSP}SEK`);
    });
  });

  test('the same amount reads differently in fr-FR and en-US', () => {
    const us = en(() => fmtMoney(1000000.25, 'EUR'));
    const fra = fr(() => fmtMoney(1000000.25, 'EUR'));
    expect(us).toBe('€1,000,000.25');
    expect(fra).toBe(`€1${NNBSP}000${NNBSP}000,25`);
  });
});

describe('compact money', () => {
  test('market-cap cells compact to B, M and K, in USD', () => {
    expect(fmtCompactMoney(263.1e9)).toBe('$263.1B');
    expect(fmtCompactMoney(172e6)).toBe('$172M');
    expect(fmtCompactMoney(-4500)).toBe(`${MINUS}$5K`);
    expect(fmtCompactMoney(0)).toBe('$0');
  });

  test('an axis label keeps the series currency', () => {
    expect(fmtCompactValue(260000, 'EUR')).toBe('€260K');
    expect(fmtCompactValue(1.2e6, 'JPY')).toBe('¥1.2M');
    expect(fmtCompactValue(12000, 'CHF')).toBe(`CHF${NBSP}12K`);
    expect(fmtCompactValue(12000, 'SEK')).toBe(`SEK${NBSP}12K`);
    expect(fmtCompactValue(-3.5e9, 'USD')).toBe(`${MINUS}$3.5B`);
    expect(fmtCompactValue(42.5, 'USD')).toBe('$42.50');
    expect(fmtCompactValue(0, 'USD')).toBe('$0');
  });
});

describe('percentages', () => {
  test('fmtPct takes percent, fmtRatioPct takes a fraction: same text for 5 and 0.05', () => {
    expect(fmtPct(5)).toBe('5.00%');
    expect(fmtRatioPct(0.05)).toBe('5.00%');
    expect(fmtRatioPct(-0.1234)).toBe(`${MINUS}12.34%`);
    expect(fmtPct(-3.456, 1)).toBe(`${MINUS}3.5%`);
    expect(fmtPct(0)).toBe('0.00%');
  });

  test('a PnL percent always carries its sign', () => {
    expect(fmtSignedPct(2.5)).toBe('+2.50%');
    expect(fmtSignedPct(-2.5)).toBe(`${MINUS}2.50%`);
    expect(fmtSignedPct(0)).toBe('+0.00%');
    expect(fmtSignedPct(12345.678, 1)).toBe('+12345.7%');
  });

  test('fmtPct rounds a half like money does, 2.675 shows 2.68%', () => {
    expect(fmtPct(2.675)).toBe('2.68%');
    expect(fmtPct(1.005)).toBe('1.01%');
  });

  test('percentages follow the browser locale, fr-FR shows 5,00%', () => {
    fr(() => {
      expect(fmtFixed(5)).toBe('5,00');
      expect(fmtPct(5)).toBe('5,00%');
    });
  });
});

describe('plain numbers', () => {
  test('fmtNum groups thousands and caps decimals, en-US and fr-FR', () => {
    en(() => {
      expect(fmtNum(1234567.891)).toBe('1,234,567.89');
      expect(fmtNum(-0.5)).toBe(`${MINUS}0.5`);
      expect(fmtNum(0)).toBe('0');
      expect(fmtNum(1e21)).toBe('1,000,000,000,000,000,000,000');
    });
    fr(() => expect(fmtNum(1234567.891)).toBe(`1${NNBSP}234${NNBSP}567,89`));
  });

  test('fmtFixed pads to a fixed count of decimals so a price column aligns', () => {
    en(() => {
      expect(fmtFixed(1400)).toBe('1,400.00');
      expect(fmtFixed(-0.1, 3)).toBe(`${MINUS}0.100`);
    });
    fr(() => expect(fmtFixed(-0.1, 3)).toBe(`${MINUS}0,100`));
  });

  test('byte sizes run to TB and tell 0 from missing', () => {
    expect(fmtBytes(0)).toBe('0 B');
    expect(fmtBytes(1536)).toBe('1.5 KB');
    expect(fmtBytes(150 * 1024 ** 2)).toBe('150 MB');
    expect(fmtBytes(5 * 1024 ** 4)).toBe('5.0 TB');
  });

  test('durations are H:MM:SS and never negative', () => {
    expect(fmtDuration(3725)).toBe('1:02:05');
    expect(fmtDuration(-10)).toBe('0:00:00');
    expect(fmtDuration(null)).toBe('0:00:00');
  });
});

describe('dates', () => {
  afterEach(() => {
    locale.set('en');
    vi.useRealTimers();
  });

  test('a date-only string is the local calendar day, not a UTC instant', () => {
    expect(dateKey('2026-03-14')).toBe('2026-03-14');
    expect(fmtDate('2026-03-14')).toBe('Mar 14, 2026');
  });

  test('dates follow the language picked in the app', async () => {
    locale.set('fr');
    // The French pack is a lazy chunk; dates only need the locale tag.
    expect(fmtDate('2026-03-14')).toBe('14 mars 2026');
    locale.set('en');
    expect(fmtDate('2026-03-14')).toBe('Mar 14, 2026');
  });

  test('an instant renders in the local zone (UTC in this suite)', () => {
    expect(fmtDateTime('2026-03-14T08:30:00Z')).toBe('Mar 14, 2026, 8:30 AM');
  });

  test('an unreadable date renders the em dash and an empty key', () => {
    expect(fmtDate('not a date')).toBe(EM_DASH);
    expect(fmtDate('')).toBe(EM_DASH);
    expect(dateKey('nope')).toBe('');
  });

  test('relative time counts back from a frozen clock', () => {
    vi.useFakeTimers({ now: new Date('2026-03-14T12:00:00Z') });
    expect(relativeTime('2026-03-14T11:59:30Z')).toEqual({ key: 'common.time.justNow', params: {} });
    expect(relativeTime('2026-03-14T11:57:00Z')).toEqual({ key: 'common.time.minsAgo', params: { mins: 3 } });
    expect(relativeTime('2026-03-14T07:00:00Z')).toEqual({ key: 'common.time.hoursAgo', params: { hrs: 5 } });
    expect(relativeTime('2026-03-10T12:00:00Z')).toEqual({ key: 'common.time.daysAgo', params: { days: 4 } });
    // A future stamp (clock skew) is "just now", not a negative age.
    expect(relativeTime('2026-03-14T12:05:00Z')).toEqual({ key: 'common.time.justNow', params: {} });
    expect(get(ago)('2026-03-14T11:57:00Z')).toBe('3m ago');
  });

  test('a compact age steps through minutes, hours, days, weeks, months and years', () => {
    const now = Date.parse('2026-03-14T12:00:00Z');
    const at = (ms) => new Date(now - ms).toISOString();
    const H = 3600_000;
    const D = 24 * H;
    expect(shortAge(at(30_000), now).key).toBe('common.time.short.now');
    expect(shortAge(at(5 * 60_000), now)).toEqual({ key: 'common.time.short.min', params: { n: 5 } });
    expect(shortAge(at(3 * H), now)).toEqual({ key: 'common.time.short.hour', params: { n: 3 } });
    expect(shortAge(at(2 * D), now)).toEqual({ key: 'common.time.short.day', params: { n: 2 } });
    expect(shortAge(at(21 * D), now)).toEqual({ key: 'common.time.short.week', params: { n: 3 } });
    expect(shortAge(at(120 * D), now)).toEqual({ key: 'common.time.short.month', params: { n: 4 } });
    expect(shortAge(at(400 * D), now)).toEqual({ key: 'common.time.short.year', params: { n: 1 } });
  });
});
