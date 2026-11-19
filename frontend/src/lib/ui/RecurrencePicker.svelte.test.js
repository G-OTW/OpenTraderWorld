// The shared recurrence picker through its three owners: the reminder form, the routine
// schedule and the automator rule. Each edit must land in the owner's own payload shape.
import { expect, test } from 'vitest';
import { render, screen, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { http, HttpResponse } from 'msw';
import { server } from '../../mocks/node.js';
import ReminderForm from '$lib/modules/remindme/ReminderForm.svelte';
import AutomatorSchedulePicker from '$lib/modules/automator/SchedulePicker.svelte';
import RecurrencePicker from './RecurrencePicker.svelte';
import { maskForDate } from '$lib/modules/remindme/api.js';

const kindGroup = () => screen.getByRole('group', { name: 'Repeats' });
const dayGroup = () => screen.getByRole('group', { name: 'Days of the week' });
const monthGroup = () => screen.getByRole('group', { name: 'Pick the days of the month' });
const kindButtons = () => within(kindGroup()).getAllByRole('button');

test('reminder: Weekly picks the start day, a day toggle adds to it, the payload is the old one', async () => {
  const user = userEvent.setup();
  let sent = null;
  render(ReminderForm, { onsubmit: (p) => (sent = p) });
  await user.type(screen.getAllByRole('textbox')[0], 'Review');
  // Kinds in the reminder order: once, daily, weekly, monthly, yearly.
  expect(kindButtons()).toHaveLength(5);
  await user.click(kindButtons()[2]);
  const start = screen.getByDisplayValue(/^\d{4}-\d{2}-\d{2}$/).value;
  const startMask = maskForDate(start);
  const days = within(dayGroup()).getAllByRole('button', { pressed: true });
  expect(days).toHaveLength(1);
  const sunday = within(dayGroup()).getAllByRole('button')[6];
  await user.click(sunday);
  await user.click(screen.getByRole('button', { name: 'Add reminder' }));
  expect(sent).toMatchObject({
    name: 'Review',
    kind: 'custom',
    linked_id: null,
    frequency: 'weekly',
    weekdays: startMask | (startMask === 64 ? 0 : 64),
    start_time: '09:00',
    end_date: null,
    max_count: null,
    active: true
  });
});

test('reminder: an edited once reminder saves its fields unchanged', async () => {
  const user = userEvent.setup();
  let sent = null;
  const initial = {
    name: 'Pay',
    kind: 'custom',
    linked_id: null,
    details: 'd',
    url: '',
    link_label: '',
    frequency: 'once',
    weekdays: null,
    start_date: '2099-01-02',
    start_time: '07:15',
    end_date: null,
    max_count: null,
    active: true
  };
  render(ReminderForm, { initial, onsubmit: (p) => (sent = p) });
  await user.click(screen.getByRole('button', { name: 'Save' }));
  const { tz_offset_minutes, ...rest } = sent;
  expect(typeof tz_offset_minutes).toBe('number');
  expect(rest).toEqual({ ...initial, start_date: '2099-01-02', start_time: '07:15' });
});

test('automator: the rule object keeps its identity and other fields; moved fields are written', async () => {
  server.use(http.get('*/api/automator/timezones', () => HttpResponse.json({ timezones: ['UTC', 'Europe/Paris'] })));
  const user = userEvent.setup();
  const rule = $state({ name: 'bot', kind: 'daily', timezone: 'UTC', at_hour: 9, at_minute: 30, weekdays: 31, day_of_month: 1, every_minutes: 60, run_at: null });
  render(AutomatorSchedulePicker, { rule });
  expect(screen.getByDisplayValue('09:30')).toBeInTheDocument();
  await user.click(kindButtons()[2]); // weekly
  expect(rule.kind).toBe('weekly');
  expect(rule.name).toBe('bot');
  await user.click(screen.getByRole('button', { name: 'Weekend' }));
  expect(rule.weekdays).toBe(96);
});

test('picker: the last weekday cannot be cleared, a single month day replaces the other', async () => {
  const user = userEvent.setup();
  let last = null;
  const kinds = [{ value: 'weekly', label: 'W' }, { value: 'monthly', label: 'M' }];
  const { rerender } = render(RecurrencePicker, {
    value: { kind: 'weekly', weekdays: 1 },
    kinds,
    onchange: (m) => (last = m)
  });
  const mon = within(dayGroup()).getAllByRole('button')[0];
  await user.click(mon);
  expect(last).toBe(null);
  await rerender({ value: { kind: 'monthly', monthDays: [1] }, kinds, monthDays: 'single', onchange: (m) => (last = m) });
  await user.click(within(monthGroup()).getByRole('button', { name: '12' }));
  expect(last.monthDays).toEqual([12]);
});
