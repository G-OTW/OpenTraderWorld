import { describe, expect, it } from 'vitest';
import { stem, tokenize, wordClouds } from './text.js';

describe('tokenize', () => {
  it('drops grammar words and trading vocabulary', () => {
    const out = tokenize("J'ai trop tradé aujourd'hui, le marché était bas puis haut : impatience et peur");
    expect(out).toEqual(['impatience', 'peur']);
  });

  it('drops every variant of a trading word', () => {
    expect(tokenize('trades trading traders positions bougies')).toEqual([]);
  });

  it('keeps feelings', () => {
    expect(tokenize('Stressé, stress, fatigue')).toEqual(['stressé', 'stress', 'fatigue']);
  });
});

describe('stem', () => {
  it('groups stress variants', () => {
    const k = stem('stress');
    for (const w of ['stressé', 'stressée', 'stressés', 'stressed', 'stresses', 'stresser']) expect(stem(w)).toBe(k);
  });

  it('groups fatigue variants', () => {
    expect(stem('fatigué')).toBe(stem('fatigue'));
    expect(stem('fatigues')).toBe(stem('fatigue'));
  });

  it('leaves short words alone', () => {
    expect(stem('peur')).toBe('peur');
    expect(stem('need')).toBe('need');
  });
});

describe('wordClouds', () => {
  it('uses the user list instead of the default one', () => {
    const days = [
      { score: 1, texts: [{ text: 'trade fatigue' }] },
      { score: 1, texts: [{ text: 'trades fatigué' }] }
    ];
    const isGood = (d) => d.score > 5;
    expect(wordClouds(days, isGood).bad.map((w) => w.word)).toEqual(['fatigue']);
    expect(wordClouds(days, isGood, { ignored: ['fatigue'] }).bad.map((w) => w.word)).toEqual(['trade']);
  });

  it('counts stem variants as one word, shown in the most written form', () => {
    const days = [
      { score: 1, texts: [{ text: 'stress' }] },
      { score: 1, texts: [{ text: 'stressé' }] },
      { score: 1, texts: [{ text: 'stress et stressé' }] }
    ];
    const { bad } = wordClouds(days, (d) => d.score > 5);
    expect(bad).toEqual([{ word: 'stress', n: 3 }]);
  });
});
