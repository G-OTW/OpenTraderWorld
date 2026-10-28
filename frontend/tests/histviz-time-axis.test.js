import { test } from 'node:test';
import assert from 'node:assert/strict';
import { timeTicks, makeStampFmt } from '../src/lib/modules/histviz/timeAxis.js';

const minutes = (start, step, n) =>
  Array.from({ length: n }, (_, k) => new Date(Date.parse(start) + k * step * 60000).toISOString());
const texts = (m) => [...m.values()].map((t) => (t.strong ? `[${t.text}]` : t.text));

test('intraday labels sit on round steps and name only what changed', () => {
  const cats = minutes('2026-10-20T00:00:00Z', 15, 900);
  const out = texts(timeTicks(cats, 0, 899, 1400, 10, true, 'UTC'));
  assert.deepEqual(out.slice(0, 4), ['12:00', '[Wed 21]', '12:00', '[Thu 22]']);
});

test('a session chart spends no room on nights and weekends', () => {
  const cats = [];
  for (let d = 1; d <= 20; d++) {
    const day = Date.UTC(2026, 9, d);
    if ([0, 6].includes(new Date(day).getUTCDay())) continue;
    for (let k = 0; k < 13; k++) cats.push(new Date(day + (13.5 * 60 + 30 * k) * 60000).toISOString());
  }
  const out = texts(timeTicks(cats, 0, cats.length - 1, 1400, 10, true, 'America/New_York'));
  assert.deepEqual(out.slice(0, 3), ['[Fri 2]', '[Mon 5]', '[Tue 6]']);
});

test('the clock follows the zone, DST fall-back included', () => {
  const cats = minutes('2026-10-24T20:30:00Z', 30, 16);
  const out = texts(timeTicks(cats, 0, 15, 2000, 10, true, 'Europe/Paris'));
  assert.deepEqual(out, ['23:00', '23:30', '[Sun 25]', '00:30', '01:00', '01:30', '02:00', '02:30', '02:00', '02:30', '03:00', '03:30', '04:00', '04:30', '05:00']);
});

test('daily stamps are dates: years and months, never shifted by the zone', () => {
  const cats = [];
  for (let k = 0; k < 2000; k++) {
    const t = Date.UTC(2019, 0, 1) + k * 86400000;
    if (![0, 6].includes(new Date(t).getUTCDay())) cats.push(new Date(t).toISOString().slice(0, 10) + 'T00:00:00Z');
  }
  const all = texts(timeTicks(cats, 0, cats.length - 1, 1400, 10, false, 'America/Los_Angeles'));
  assert.deepEqual(all.slice(0, 2), ['[Jul]', '[2020]']);
  const recent = texts(timeTicks(cats, cats.length - 120, cats.length - 1, 1400, 10, false, 'UTC'));
  assert.ok(recent.includes('[Mar]') && recent.includes('16'));
});

test('a larger unit wins a collision', () => {
  // 1-minute bars across midnight on a narrow axis: the day label survives.
  const cats = minutes('2026-10-25T23:50:00Z', 1, 20);
  const out = texts(timeTicks(cats, 0, 19, 120, 10, true, 'UTC'));
  assert.ok(out.includes('[Mon 26]'));
});

test('crosshair stamp is readable', () => {
  assert.equal(makeStampFmt('Europe/Paris', true)('2026-10-25T05:56:00Z'), "Sun 25 Oct '26 06:56");
  assert.equal(makeStampFmt('America/Los_Angeles', false)('2026-10-25T00:00:00Z'), 'Sun 25 Oct 2026');
  assert.equal(makeStampFmt('UTC', true)('+3'), '+3');
});
