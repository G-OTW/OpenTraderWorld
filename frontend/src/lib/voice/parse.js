/**
 * Spoken text → plan. Pure functions, no DOM, no store: the voice store feeds them the
 * transcript, the saved commands and the navigation targets, and runs what comes back.
 *
 * The sentence is cut into pieces on chain words ("and", "then", "et", "puis"…) and on
 * commas. A saved command fires only when a run of whole pieces equals one of its phrases,
 * never on a word inside a piece: "a blue turtle" does not trigger "turtle". Runs are tried
 * longest first, so a phrase that itself contains "and" still matches whole.
 *
 * What no command claims is tried as a built-in "open <page>", and what is left goes to
 * the agent as one prompt (adjacent leftovers are joined back, so "compare AAPL and MSFT"
 * reaches the agent intact). An "open" that names several pages is an error that lists
 * them: the app never picks one for the user.
 */

/** Chain words per language. Multi-word entries are matched before their prefixes. */
const SEPARATORS = {
  en: ['and then', 'then', 'and', 'after that'],
  fr: ['et ensuite', 'et puis', 'ensuite', 'puis', 'et', 'apres ca'],
  de: ['und dann', 'dann', 'und', 'danach'],
  es: ['y luego', 'y despues', 'luego', 'despues', 'y'],
  it: ['e poi', 'poi', 'quindi', 'e'],
  pt: ['e depois', 'e entao', 'depois', 'entao', 'e'],
  zh: ['然后', '并且', '接着']
};

/** "Open" verbs per language, normalized. The target follows. */
const OPEN_VERBS = {
  en: ['open', 'go to', 'show me', 'show', 'switch to', 'take me to', 'navigate to'],
  fr: ['ouvre', 'ouvrir', 'va sur', 'va a', 'va dans', 'aller a', 'aller sur', 'affiche', 'montre moi', 'montre'],
  de: ['offne', 'oeffne', 'zeige', 'zeig', 'gehe zu', 'geh zu', 'wechsle zu'],
  es: ['abre', 'abrir', 'ir a', 've a', 'muestra', 'muestrame'],
  it: ['apri', 'vai a', 'vai su', 'mostra', 'mostrami'],
  pt: ['abra', 'abre', 'abrir', 'ir para', 'va para', 'mostra', 'mostre'],
  zh: ['打开', '显示', '去']
};

/** Articles and possessives allowed between the verb and the page name. */
const FILLERS = new Set([
  'the', 'my', 'le', 'la', 'les', 'l', 'mon', 'ma', 'mes', 'du', 'de', 'des', 'der', 'die',
  'das', 'den', 'mein', 'meine', 'el', 'los', 'las', 'mi', 'mis', 'il', 'lo', 'gli', 'o',
  'os', 'a', 'as', 'meu', 'minha', 'page', 'pagina', 'seite', 'module', 'modulo', 'modul'
]);

/** Language code (`fr-FR` → `fr`), falling back to English vocabularies. */
export function langOf(code) {
  const base = String(code || '').toLowerCase().split(/[-_]/)[0];
  return SEPARATORS[base] ? base : 'en';
}

/** Lowercase, accents stripped, letters and digits only, single spaces. */
export function normalize(s) {
  return String(s ?? '')
    .normalize('NFD')
    .replace(/\p{M}+/gu, '')
    .toLowerCase()
    .replace(/[^\p{L}\p{N}]+/gu, ' ')
    .trim()
    .replace(/\s+/g, ' ');
}

const isCjk = (s) => /[㐀-鿿]/.test(s);

/**
 * Cut a transcript into pieces. Each piece keeps its original text (sent to the agent as
 * spoken) and its normalized form (matched against phrases), plus the separator that
 * followed it so leftovers can be joined back.
 */
export function splitPieces(text, lang = 'en') {
  const raw = String(text ?? '').trim();
  if (!raw) return [];
  const seps = (SEPARATORS[langOf(lang)] ?? SEPARATORS.en)
    .map((s) => normalize(s).split(' '))
    .sort((a, b) => b.length - a.length);

  // Chinese has no spaces: cut on the separator characters and punctuation directly.
  if (isCjk(raw) && !/\s/.test(raw)) {
    const parts = raw.split(/(然后|并且|接着|，|,|。|；|;)/);
    const out = [];
    for (let i = 0; i < parts.length; i += 2) {
      const piece = parts[i].trim();
      if (piece) out.push({ text: piece, norm: normalize(piece), sep: (parts[i + 1] ?? '').trim() });
    }
    return out;
  }

  const words = raw.split(/\s+/);
  const out = [];
  let cur = [];
  const flush = (sep) => {
    const piece = cur.join(' ').replace(/[,;.!?]+$/, '').trim();
    if (piece && normalize(piece)) out.push({ text: piece, norm: normalize(piece), sep });
    else if (out.length && sep) out[out.length - 1].sep = sep;
    cur = [];
  };
  for (let i = 0; i < words.length; i++) {
    const hit = seps.find((sep) =>
      sep.every((w, k) => i + k < words.length && normalize(words[i + k]) === w)
    );
    // A chain word only splits between two things: never the first word of the sentence.
    // Right after a comma it just joins the comma ("trades, then go").
    if (hit && (cur.length || out.length)) {
      const spoken = words.slice(i, i + hit.length).join(' ');
      if (cur.length) flush(spoken);
      else out[out.length - 1].sep = spoken;
      i += hit.length - 1;
      continue;
    }
    cur.push(words[i]);
    if (/[,;]$/.test(words[i])) flush(',');
  }
  flush('');
  return out;
}

/** Index saved commands by every normalized phrasing. Disabled commands are left out. */
export function indexCommands(commands) {
  const map = new Map();
  for (const c of commands ?? []) {
    if (!c.enabled) continue;
    for (const p of [c.phrase, ...(c.aliases ?? [])]) {
      const n = normalize(p);
      if (n && !map.has(n)) map.set(n, c);
    }
  }
  return map;
}

/**
 * Match an "open <page>" piece against the navigation targets
 * (`[{ label, path, keys: [normalized names] }]`). Returns `{ target }`, `{ ambiguous }`,
 * or null when the piece is not an "open" at all or names nothing known.
 */
export function matchOpen(norm, targets, lang = 'en') {
  const verbs = [...(OPEN_VERBS[langOf(lang)] ?? []), ...(langOf(lang) === 'en' ? [] : OPEN_VERBS.en)]
    .map(normalize)
    .sort((a, b) => b.length - a.length);
  const verb = verbs.find((v) => norm === v || norm.startsWith(v + ' ') || (isCjk(v) && norm.startsWith(v)));
  if (!verb) return null;
  let rest = norm.slice(verb.length).trim().split(' ').filter(Boolean);
  while (rest.length > 1 && FILLERS.has(rest[0])) rest = rest.slice(1);
  const want = rest.join(' ');
  if (!want) return null;

  // An exact name wins outright; otherwise every spoken word must appear in the name.
  const exact = targets.filter((tg) => tg.keys.includes(want));
  if (exact.length === 1) return { target: exact[0] };
  const words = want.split(' ');
  const loose = (exact.length ? exact : targets).filter((tg) =>
    tg.keys.some((k) => {
      const kw = k.split(' ');
      return words.every((w) => kw.includes(w));
    })
  );
  if (loose.length === 1) return { target: loose[0] };
  if (loose.length > 1) return { ambiguous: loose, spoken: want };
  return null;
}

/**
 * Build the plan for a transcript.
 *
 * Returns `{ entries, steps, confirm, error }`:
 * - `entries`: what each part of the sentence became (`command` | `open` | `agent` |
 *   `unknown` | `ambiguous`), for the panel to show where each step came from.
 * - `steps`: the flat, ordered list to run.
 * - `confirm`: false only when every part is a saved command that bypasses confirmation.
 * - `error`: set when some part cannot run; the plan is then shown, never run.
 */
export function buildPlan(text, { commands = [], targets = [], lang = 'en', agentFallback = true } = {}) {
  const pieces = splitPieces(text, lang);
  const byPhrase = indexCommands(commands);
  const entries = [];
  let i = 0;
  while (i < pieces.length) {
    let matched = false;
    for (let j = pieces.length - 1; j >= i; j--) {
      const run = pieces.slice(i, j + 1);
      const norm = run.map((p, k) => (k < run.length - 1 ? `${p.norm} ${normalize(p.sep)}` : p.norm)).join(' ').trim();
      const cmd = byPhrase.get(norm);
      if (cmd) {
        entries.push({ type: 'command', text: joinPieces(run), command: cmd, steps: cmd.steps ?? [] });
        i = j + 1;
        matched = true;
        break;
      }
    }
    if (matched) continue;
    const open = matchOpen(pieces[i].norm, targets, lang);
    if (open?.target) {
      entries.push({
        type: 'open',
        text: pieces[i].text,
        steps: [{ kind: 'navigate', target: open.target.path, label: open.target.label }]
      });
    } else if (open?.ambiguous) {
      entries.push({ type: 'ambiguous', text: pieces[i].text, spoken: open.spoken, candidates: open.ambiguous });
    } else {
      // Leftovers next to each other are one request, rejoined with the words that split them.
      const prev = entries[entries.length - 1];
      if (prev?.free) {
        prev.pieces.push(pieces[i]);
      } else {
        entries.push({ free: true, pieces: [pieces[i]] });
      }
    }
    i++;
  }

  for (const e of entries) {
    if (!e.free) continue;
    e.text = joinPieces(e.pieces);
    delete e.free;
    delete e.pieces;
    if (agentFallback) {
      e.type = 'agent';
      e.steps = [{ kind: 'agent', prompt: e.text }];
    } else {
      e.type = 'unknown';
    }
  }

  const steps = entries.flatMap((e) => e.steps ?? []);
  const bad = entries.find((e) => e.type === 'unknown' || e.type === 'ambiguous');
  const confirm = !(entries.length && entries.every((e) => e.type === 'command' && e.command.bypass_confirm));
  return {
    entries,
    steps,
    confirm,
    error: !entries.length ? 'empty' : bad ? bad.type : null
  };
}

function joinPieces(run) {
  return run
    .map((p, k) => (k < run.length - 1 && p.sep ? `${p.text}${p.sep === ',' ? ',' : ` ${p.sep}`}` : p.text))
    .join(' ');
}

/**
 * Navigation targets from installed modules, plus settings and the dashboard. Each target
 * answers to its display name, its id, and the name without a leading article.
 */
export function navTargets(mods, extra = []) {
  const out = [];
  for (const m of mods) {
    const keys = new Set([normalize(m.name), normalize(m.id.replace(/-/g, ' '))]);
    for (const alt of m.voice ?? []) keys.add(normalize(alt));
    out.push({ label: m.name, path: m.base, keys: [...keys].filter(Boolean) });
  }
  for (const x of extra) out.push({ ...x, keys: x.keys.map(normalize).filter(Boolean) });
  return out;
}
