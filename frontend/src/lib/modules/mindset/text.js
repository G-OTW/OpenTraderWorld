/** Free-text and tag analysis for the mindset dashboard.
 *
 * No model, no sentiment: the text is read against the day's own score (and P&L when a
 * journal is chosen). A word is "of bad days" because it shows up on days that scored
 * low, not because a lexicon says it is negative. Pure functions, like scoring.js. */

// Small stopword sets; enough to keep "the / de / que" out of the lists. Accents kept, so
// "marché" and "marche" stay distinct.
const STOP = new Set(
  (
    'the and for with that this was were are you your have has had not but all too very ' +
    'into from out over just more than then them they their there here what when which who ' +
    'will would could should about after before because been being can did does doing ' +
    'les des une est pas que qui dans pour sur avec son ses mes mon mais tout tous cette ' +
    'cela ceci ils elles nous vous etre été sont ont fait faire plus moins aussi comme bien ' +
    'par vers chez sans sous entre donc alors ainsi trop très peu beaucoup encore deja déjà ' +
    'quoi dont leur leurs cet ces elle lui aux ete jai cest est-ce après apres avant pendant ' +
    'depuis jamais toujours puis donc même meme autre autres chaque quand comment pourquoi'
  ).split(/\s+/)
);

export function tokenize(text) {
  if (typeof text !== 'string') return [];
  return text
    .toLowerCase()
    .split(/[^\p{L}\p{N}'’-]+/u)
    .map((w) => w.replace(/^[-'’]+|[-'’]+$/g, '').replace(/^(l|d|j|c|n|s|qu|m|t)['’]/, ''))
    .filter((w) => w.length >= 3 && !STOP.has(w) && !/^\d+$/.test(w));
}

const mean = (xs) => (xs.length ? xs.reduce((a, b) => a + b, 0) / xs.length : null);

/** Attach P&L to a group of days and summarise it. `pnlByDate` may be empty. */
function group(days, pnlByDate) {
  const traded = days.filter((d) => pnlByDate.has(d.date));
  return {
    n: days.length,
    score: mean(days.map((d) => d.score)),
    traded: traded.length,
    pnl: mean(traded.map((d) => pnlByDate.get(d.date))),
    win: traded.length ? traded.filter((d) => pnlByDate.get(d.date) > 0).length / traded.length : null,
    dates: days.map((d) => d.date)
  };
}

/** Per tag: how many days it was picked, their mean score, their P&L. Sorted worst score
 *  first, so the tag that goes with the bad days tops the list. */
export function tagStats(days, pnlByDate = new Map(), minDays = 2) {
  const byTag = new Map();
  for (const d of days) {
    for (const tg of new Set(d.tags ?? [])) {
      if (!byTag.has(tg)) byTag.set(tg, []);
      byTag.get(tg).push(d);
    }
  }
  return [...byTag.entries()]
    .filter(([, ds]) => ds.length >= minDays)
    .map(([tag, ds]) => ({ tag, ...group(ds, pnlByDate) }))
    .sort((a, b) => a.score - b.score);
}

/** Plain text of a ProseMirror document (the day note): every text node, in order. */
export function docText(doc) {
  const out = [];
  const walk = (n) => {
    if (!n || typeof n !== 'object') return;
    if (typeof n.text === 'string') out.push(n.text);
    if (Array.isArray(n.content)) n.content.forEach(walk);
  };
  walk(doc);
  return out.join(' ');
}

/** Two word clouds: the words that repeat on "good" days and on "bad" days, where
 *  `isGood(day)` decides the side. A word counts once per day; its weight is the number
 *  of days it was written on. Words seen on a single day are noise and stay out. */
export function wordClouds(days, isGood, { minDays = 2, top = 40 } = {}) {
  const good = new Map();
  const bad = new Map();
  let docs = 0;
  for (const d of days) {
    const side = isGood(d);
    if (side == null) continue;
    const words = new Set((d.texts ?? []).flatMap((t) => tokenize(t.text)));
    if (words.size === 0) continue;
    docs++;
    const m = side ? good : bad;
    for (const w of words) m.set(w, (m.get(w) ?? 0) + 1);
  }
  const list = (m) =>
    [...m.entries()]
      .filter(([, n]) => n >= minDays)
      .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
      .slice(0, top)
      .map(([word, n]) => ({ word, n }));
  return { docs, good: list(good), bad: list(bad) };
}
