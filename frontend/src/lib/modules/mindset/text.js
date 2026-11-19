/** Free-text and tag analysis for the mindset dashboard.
 *
 * No model, no sentiment: the text is read against the day's own score (and P&L when a
 * journal is chosen). A word is "of bad days" because it shows up on days that scored
 * low, not because a lexicon says it is negative. Pure functions, like scoring.js. */

// Grammar words (fr/en): articles, prepositions, pronouns, auxiliaries, fillers.
const STOP = new Set(
  (
    'the and for with that this was were are you your have has had not but all too very ' +
    'into from out over just more than then them they their there here what when which who ' +
    'will would could should about after before because been being can did does doing ' +
    'its our his her she him how why where also some any much many only still even again ' +
    'got get getting went one two off now yes really thing things lot bit way well ' +
    "i'm it's don't didn't can't i've won't wasn't isn't that's " +
    'les des une uns unes est pas que qui dans pour sur avec son ses mes mon mais tout tous ' +
    'toute toutes cette cela ceci ils elles nous vous etre être été sont ont fait faire plus ' +
    'moins aussi comme bien par vers chez sans sous entre donc alors ainsi trop très peu ' +
    'beaucoup encore deja déjà quoi dont leur leurs cet ces elle lui aux eux ete jai cest ' +
    'est-ce après apres avant pendant depuis jamais toujours puis même meme autre autres ' +
    'chaque quand comment pourquoi notre votre nos vos tes ton car parce lorsque puisque ' +
    'selon contre durant malgré dès lors tant tel telle tels sinon ensuite enfin presque ' +
    'plutôt surtout assez juste vraiment fois chose choses quelque quelques celle celui ceux ' +
    'ici oui non rien suis sommes était étais étaient avais avait avaient sera serai vais ' +
    'peut peux faut dois doit veux voulais pouvais avoir ça ceux-ci celle-ci là-dessus ' +
    "aujourd'hui"
  ).split(/\s+/)
);

// Trading words present in almost every answer: they say nothing about the state of mind.
// The default of the user's ignored-words list (pref `mindset.cloudIgnore`). Matched on
// the stem too, so "trades / trading / tradé" go with "trade".
export const DEFAULT_IGNORED = (
  'trade trader trading tradé position entrée entry sortie exit prix price marché market ' +
  'achat buy vente sell long short haut bas high low hausse baisse jour journée day today ' +
  'demain hier tomorrow yesterday matin soir morning evening session ordre order bougie ' +
  'candle chart graphique point tick contrat contract lot heure hour minute semaine week'
).split(/\s+/);

/** Pref key of the ignored-words list; unset means `DEFAULT_IGNORED`. */
export const IGNORED_PREF = 'mindset.cloudIgnore';

/** Light fr/en stemmer: one suffix off, then "ss" folded, so "stress / stressé / stressed"
 *  share one key. The stem is a grouping key only, never shown. */
const SUFFIXES = ['ements', 'ement', 'ings', 'ing', 'ées', 'ée', 'és', 'é', 'ers', 'er', 'ed', 'es', 'e', 's'];
export function stem(w) {
  let s = w;
  for (const suf of SUFFIXES) {
    if (s.endsWith(suf) && s.length - suf.length >= 3) {
      s = s.slice(0, -suf.length);
      break;
    }
  }
  return s.endsWith('ss') ? s.slice(0, -1) : s;
}

/** Ignored words as lookup sets, by surface form and by stem. */
export function ignoreSet(words = DEFAULT_IGNORED) {
  const list = words.map((w) => String(w).trim().toLowerCase().replace(/’/g, "'")).filter(Boolean);
  return { words: new Set(list), stems: new Set(list.map(stem)) };
}

const DEFAULT_SET = ignoreSet();

export function tokenize(text, ignore = DEFAULT_SET) {
  if (typeof text !== 'string') return [];
  return text
    .toLowerCase()
    .replace(/’/g, "'")
    .split(/[^\p{L}\p{N}'-]+/u)
    .map((w) => w.replace(/^[-']+|[-']+$/g, '').replace(/^(l|d|j|c|n|s|qu|m|t)'/, ''))
    .filter(
      (w) =>
        w.length >= 3 &&
        !STOP.has(w) &&
        !/^\d+$/.test(w) &&
        !ignore.words.has(w) &&
        !ignore.stems.has(stem(w))
    );
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
 *  of days it was written on. Words seen on a single day are noise and stay out.
 *  Variants of one stem ("stress", "stressé") count as one word, shown in the form
 *  written most often. `ignored` is the user's list (null = the default one). */
export function wordClouds(days, isGood, { minDays = 2, top = 40, ignored = null } = {}) {
  const ignore = ignored ? ignoreSet(ignored) : DEFAULT_SET;
  const good = new Map();
  const bad = new Map();
  const forms = new Map(); // stem -> Map(surface form -> uses)
  let docs = 0;
  for (const d of days) {
    const side = isGood(d);
    if (side == null) continue;
    const tokens = (d.texts ?? []).flatMap((t) => tokenize(t.text, ignore));
    if (tokens.length === 0) continue;
    docs++;
    const keys = new Set();
    for (const w of tokens) {
      const k = stem(w);
      keys.add(k);
      if (!forms.has(k)) forms.set(k, new Map());
      forms.get(k).set(w, (forms.get(k).get(w) ?? 0) + 1);
    }
    const m = side ? good : bad;
    for (const k of keys) m.set(k, (m.get(k) ?? 0) + 1);
  }
  const shown = (k) =>
    [...forms.get(k).entries()].sort((a, b) => b[1] - a[1] || a[0].length - b[0].length || a[0].localeCompare(b[0]))[0][0];
  const list = (m) =>
    [...m.entries()]
      .filter(([, n]) => n >= minDays)
      .map(([k, n]) => ({ word: shown(k), n }))
      .sort((a, b) => b.n - a.n || a.word.localeCompare(b.word))
      .slice(0, top);
  return { docs, good: list(good), bad: list(bad) };
}
