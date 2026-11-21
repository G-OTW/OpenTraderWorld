import { describe, it, expect } from 'vitest';
import { mindsetByDate, mindsetCell } from './mindsetDays.js';

const hist = {
  templates: [
    { id: 'pre', name: 'Pre', active: true },
    { id: 'post', name: 'Post', active: true },
    { id: 'off', name: 'Off', active: false }
  ],
  prompts: [
    { id: 'sleep', template_id: 'pre', kind: 'scale', config: {} },
    { id: 'tilt', template_id: 'post', kind: 'scale', config: { score: 'desc' } },
    { id: 'note', template_id: 'post', kind: 'text', config: {} }
  ],
  entries: [
    { entry_date: '2026-11-02', template_id: 'pre', answers: { sleep: 5 } },
    { entry_date: '2026-11-02', template_id: 'post', answers: { tilt: 2, note: 'calm' } },
    { entry_date: '2026-11-03', template_id: 'pre', answers: { sleep: 1 } },
    { entry_date: '2026-11-04', template_id: 'post', answers: { note: 'only text' } }
  ]
};

describe('mindset on the calendar', () => {
  const read = mindsetByDate(hist, ['pre', 'post', 'off']);

  it('keeps only picked, active templates', () => {
    expect(read.templates.map((t) => t.id)).toEqual(['pre', 'post']);
  });

  it('scores a fully filled traded day with the mindset normalization', () => {
    // sleep 5 asc = 100, tilt 2 desc = 75: mean 87.5
    expect(mindsetCell(read, '2026-11-02', true)).toMatchObject({ tone: 'done', done: 2, score: 88 });
  });

  it('reads a half filled traded day as partial', () => {
    expect(mindsetCell(read, '2026-11-03', true)).toMatchObject({ tone: 'partial', done: 1, score: 0 });
  });

  it('marks a traded day with no check-in as missed', () => {
    expect(mindsetCell(read, '2026-11-05', true)).toMatchObject({ tone: 'miss', done: 0, score: null });
  });

  it('shows nothing on an untraded day without check-in', () => {
    expect(mindsetCell(read, '2026-11-05', false)).toBeNull();
  });

  it('shows an untraded check-in without blame', () => {
    expect(mindsetCell(read, '2026-11-03', false)).toMatchObject({ tone: 'none', score: 0 });
  });

  it('has no score when nothing filled is scored', () => {
    expect(mindsetCell(read, '2026-11-04', true)).toMatchObject({ tone: 'partial', score: null });
  });

  it('shows nothing when the book picked no template', () => {
    expect(mindsetCell(mindsetByDate(hist, []), '2026-11-02', true)).toBeNull();
  });
});
