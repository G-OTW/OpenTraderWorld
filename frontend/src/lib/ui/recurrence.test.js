// The shared recurrence model and the three module adapters (reminders, routines,
// automator schedules). The contract under test: every stored shape survives a round trip
// through the model unchanged (same object, same fields, legacy gaps kept), and an edit
// writes only the fields it moved, in the shape the module already saves.
import { describe, it, expect } from 'vitest';
import { ruleKey, sameRule, patch, isoToLocalInput, localInputToIso, hhmm } from './recurrence.js';
import * as remind from '$lib/modules/remindme/recurrence.js';
import * as routines from '$lib/modules/routines/recurrence.js';
import * as automator from '$lib/modules/automator/recurrence.js';
import { reminderFromEvent, reminderPayload, blankReminder } from '$lib/modules/remindme/draft.js';

const roundTrip = (adapter, payload) => adapter.fromModel(adapter.toModel(payload), payload);

describe('shared helpers', () => {
  it('ruleKey ignores fields of other kinds', () => {
    expect(sameRule({ kind: 'weekly', weekdays: 31, every: 1, date: 'x' }, { kind: 'weekly', weekdays: 31 })).toBe(true);
    expect(sameRule({ kind: 'weekly', weekdays: 31, every: 2 }, { kind: 'weekly', weekdays: 31 })).toBe(false);
    expect(ruleKey({ kind: 'monthly', monthDays: [15, -1, 1] })).toEqual(['monthly', [-1, 1, 15]]);
    expect(ruleKey(null)).toBe(null);
  });

  it('patch returns the same object when nothing moved', () => {
    const prev = { a: 1 };
    expect(patch(prev, { x: 1 }, { x: 1 }, [['x', (m) => ({ a: m.x })]])).toBe(prev);
    expect(patch(prev, { x: 1 }, { x: 2 }, [['x', (m) => ({ a: m.x })]])).toEqual({ a: 2 });
  });

  it('datetime-local conversion is local and reversible (suite runs in UTC)', () => {
    expect(isoToLocalInput('2026-11-12T08:05:00Z')).toBe('2026-11-12T08:05');
    expect(isoToLocalInput('2026-11-12T08:05:00+00:00')).toBe('2026-11-12T08:05');
    expect(isoToLocalInput(null)).toBe('');
    expect(isoToLocalInput('garbage')).toBe('');
    expect(localInputToIso('2026-11-12T08:05')).toBe('2026-11-12T08:05:00.000Z');
    expect(localInputToIso('')).toBe(null);
    expect(hhmm(8, 5)).toBe('08:05');
  });
});

describe('remindme adapter', () => {
  const rows = [
    { frequency: 'once', weekdays: null, start_date: '2026-11-12', start_time: '09:00', end_date: null, max_count: null },
    { frequency: 'daily', weekdays: null, start_date: '2026-11-12', start_time: '07:30', end_date: '2027-01-01', max_count: 10 },
    { frequency: 'weekly', weekdays: 21, start_date: '2026-11-12', start_time: '18:00', end_date: null, max_count: null },
    // Weekly rows written before the day picker: no mask, the server falls back to the start day.
    { frequency: 'weekly', weekdays: null, start_date: '2026-11-12', start_time: '18:00', end_date: null, max_count: null },
    { frequency: 'monthly', weekdays: null, start_date: '2026-01-31', start_time: '10:00', end_date: null, max_count: 3 },
    { frequency: 'yearly', weekdays: null, start_date: '2026-03-14', start_time: '00:00', end_date: null, max_count: null },
    // Form draft: empty strings for the optional fields.
    { ...blankReminder(), name: 'x' },
    // A row missing optional fields altogether.
    { frequency: 'once', start_date: '2026-11-12' }
  ];

  it.each(rows)('round trips %o unchanged', (row) => {
    expect(roundTrip(remind, row)).toBe(row);
  });

  it('writes only the moved field', () => {
    const row = rows[2];
    const m = { ...remind.toModel(row), weekdays: 21 | 64 };
    expect(remind.fromModel(m, row)).toEqual({ ...row, weekdays: 85 });
    const k = { ...remind.toModel(row), kind: 'monthly' };
    expect(remind.fromModel(k, row)).toEqual({ ...row, frequency: 'monthly' });
    const c = { ...remind.toModel(row), maxCount: 5, end: '2027-02-01', time: '06:00' };
    expect(remind.fromModel(c, row)).toEqual({ ...row, max_count: 5, end_date: '2027-02-01', start_time: '06:00' });
  });

  it('lists the next weekly fires from the start date and stops at the end date', () => {
    const now = new Date('2026-11-11T12:00:00');
    const r = { frequency: 'weekly', weekdays: 1 | 16, start_date: '2026-11-11', start_time: '09:00', end_date: '2026-11-20' };
    const out = remind.nextFires(r, now).map((d) => d.toISOString().slice(0, 16));
    expect(out).toEqual(['2026-11-13T09:00', '2026-11-16T09:00', '2026-11-20T09:00']);
    expect(remind.nextFires({ ...r, frequency: 'daily' }, now)).toEqual([]);
  });

  it('builds the same payload the form always sent', () => {
    const p = reminderPayload({ ...blankReminder(), name: 'n', url: 'example.com', frequency: 'daily', weekdays: 4, max_count: '' });
    expect(p).toMatchObject({ name: 'n', url: 'https://example.com', weekdays: null, max_count: null, linked_id: null });
  });

  it('derives the calendar event reminder (all-day at 09:00)', () => {
    const allDay = reminderFromEvent({ title: '', start_at: '2026-11-12T00:00:00.000Z', all_day: true, notes: null }, 'Event');
    expect(allDay).toMatchObject({ name: 'Event', kind: 'custom', frequency: 'once', start_date: '2026-11-12', start_time: '09:00', details: '' });
    const timed = reminderFromEvent({ title: 'FOMC', start_at: '2026-11-12T14:30:00.000Z', all_day: false, notes: 'n' });
    expect(timed).toMatchObject({ name: 'FOMC', start_time: '14:30', details: 'n' });
  });
});

describe('routines adapter', () => {
  const shapes = [
    { schedule: { kind: 'weekly', weekdays: 31, interval: 1 }, start_date: '', end_date: '' },
    { schedule: { kind: 'weekly', weekdays: 21, interval: 2 }, start_date: '2026-11-01', end_date: '2026-12-31' },
    // A legacy weekly with no interval keeps having none.
    { schedule: { kind: 'weekly', weekdays: 127 }, start_date: null, end_date: null },
    { schedule: { kind: 'daily', interval: 3 }, start_date: '2026-11-01', end_date: '' },
    { schedule: { kind: 'monthly', days: [1, 15, -1] }, start_date: '', end_date: '2027-01-01' },
    { schedule: { kind: 'nth_weekday', weekday: 3, nth: -1 }, start_date: '', end_date: '' },
    { schedule: { kind: 'once', date: '2026-12-24' }, start_date: '', end_date: '' },
    { schedule: { kind: 'once' }, start_date: '', end_date: '' }
  ];

  it.each(shapes)('round trips %o unchanged', (p) => {
    const out = roundTrip(routines, p);
    expect(out.schedule).toBe(p.schedule);
    expect(out.start_date).toBe(p.start_date);
    expect(out.end_date).toBe(p.end_date);
  });

  it('an edit keeps the kind fields it did not touch', () => {
    const p = shapes[1];
    const out = routines.fromModel({ ...routines.toModel(p), weekdays: 1 }, p);
    expect(out.schedule).toEqual({ kind: 'weekly', weekdays: 1, interval: 2 });
    expect(out.start_date).toBe(p.start_date);
  });

  it('switching kind builds a fresh rule of that kind', () => {
    const p = shapes[0];
    const m = routines.toModel(p);
    const to = (kind) => routines.fromModel({ ...m, kind, ...routines.kindDefaults(kind, m) }, p).schedule;
    expect(to('daily')).toEqual({ kind: 'daily', interval: 1 });
    expect(to('monthly')).toEqual({ kind: 'monthly', days: [1] });
    expect(to('nth_weekday')).toEqual({ kind: 'nth_weekday', weekday: 0, nth: 1 });
    const withStart = { ...p, start_date: '2026-12-01' };
    const mm = routines.toModel(withStart);
    expect(routines.fromModel({ ...mm, kind: 'once', ...routines.kindDefaults('once', mm) }, withStart).schedule)
      .toEqual({ kind: 'once', date: '2026-12-01' });
    const back = { schedule: { kind: 'daily', interval: 2 }, start_date: '', end_date: '' };
    const bm = routines.toModel(back);
    expect(routines.fromModel({ ...bm, kind: 'weekly', ...routines.kindDefaults('weekly', bm) }, back).schedule)
      .toEqual({ kind: 'weekly', weekdays: 31, interval: 1 });
  });

  it('a preset applies its rule and keeps the window', () => {
    const p = shapes[1];
    const preset = routines.ruleOf({ kind: 'monthly', days: [-1] });
    const out = routines.fromModel({ ...routines.toModel(p), ...preset }, p);
    expect(out).toEqual({ schedule: { kind: 'monthly', days: [-1] }, start_date: p.start_date, end_date: p.end_date });
    expect(sameRule({ ...routines.toModel(shapes[0]) }, routines.ruleOf({ kind: 'weekly', weekdays: 31, interval: 1 }))).toBe(true);
  });

  it('month days are stored sorted', () => {
    const p = shapes[4];
    const out = routines.fromModel({ ...routines.toModel(p), monthDays: [15, 2, -1] }, p);
    expect(out.schedule).toEqual({ kind: 'monthly', days: [-1, 2, 15] });
  });
});

describe('automator adapter', () => {
  const base = {
    workflow_id: 'w1',
    kind: 'daily',
    timezone: 'Europe/Paris',
    every_minutes: 60,
    at_hour: 8,
    at_minute: 0,
    weekdays: 31,
    day_of_month: 1,
    run_at: null,
    catch_up: false,
    active: true
  };
  const shapes = [
    base,
    { ...base, kind: 'interval', every_minutes: 15 },
    { ...base, kind: 'weekly', weekdays: 65, at_hour: 17, at_minute: 45 },
    { ...base, kind: 'monthly', day_of_month: 31 },
    { ...base, kind: 'once', run_at: '2026-11-12T08:05:00Z' },
    { ...base, kind: 'once', run_at: '2026-11-12T08:05:00.123+00:00' },
    // A robot session: the same rule fields on a row with many others.
    { name: 'bot', feed: 'hist', kind: 'interval', timezone: 'UTC', every_minutes: 60, at_hour: 9, at_minute: 0, weekdays: 31, day_of_month: 1, run_at: null, settings: { a: 1 }, channel_ids: [] },
    // A rule with no time yet (the picker fills 08:00 on open, not the adapter).
    { kind: 'daily', timezone: 'UTC' }
  ];

  it.each(shapes)('round trips %o unchanged', (r) => {
    expect(roundTrip(automator, r)).toBe(r);
  });

  it('writes only the moved fields', () => {
    const m = automator.toModel(base);
    expect(automator.fromModel({ ...m, time: '17:45' }, base)).toEqual({ ...base, at_hour: 17, at_minute: 45 });
    expect(automator.fromModel({ ...m, kind: 'weekly' }, base)).toEqual({ ...base, kind: 'weekly' });
    expect(automator.fromModel({ ...m, monthDays: [12] }, base)).toEqual({ ...base, day_of_month: 12 });
    expect(automator.fromModel({ ...m, everyMinutes: 5 }, base)).toEqual({ ...base, every_minutes: 5 });
    expect(automator.fromModel({ ...m, weekdays: 96 }, base)).toEqual({ ...base, weekdays: 96 });
    expect(automator.fromModel({ ...m, timezone: 'UTC' }, base)).toEqual({ ...base, timezone: 'UTC' });
    expect(automator.fromModel({ ...m, runAt: '2026-12-01T10:30' }, base)).toEqual({ ...base, run_at: '2026-12-01T10:30:00.000Z' });
  });

  it('a robot session keeps every non-schedule field', () => {
    const s = shapes[6];
    const out = automator.fromModel({ ...automator.toModel(s), kind: 'daily', time: '10:00' }, s);
    expect(out).toEqual({ ...s, kind: 'daily', at_hour: 10, at_minute: 0 });
  });
});
