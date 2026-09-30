# Studying a company or instrument

## When to use

"Tell me about X", "should I look at Y", or any request for a structured view of a single name.

## Know what this platform has — and does not

**OpenTraderWorld has no financial-statements source.** Product Search (`findb`) carries
*identity*: name, ticker, kind, exchange, country, sector, currency. It does **not** carry
revenue, margins, debt, cash flow, or multiples. Historical Data carries OHLCV prices. News
carries whatever feeds the user configured.

So a "fundamental" study here is honest about a hole in the middle. Fill it only from something
real: an external MCP data source the user connected, or a filing they paste in. **Never from
memory** — you cannot know a company's current financials, and a fabricated margin is worse than
an absent one because it looks like an answer.

## Procedure

1. **Identity** — `GET /api/findb/search?q=…` to pin down the exact instrument: ticker, exchange,
   country, sector, kind. Ambiguous tickers across exchanges are common; resolve it explicitly.
2. **Price context** — `GET /api/histdata/datasets`, and download if needed (`data-acquire`).
   Range, drawdown, volatility (`risk-metrics`). This is measurable, so measure it.
3. **News timeline** — `GET /api/feed-items?search=…`, applying `news-digest` and `source-check`.
   Build a dated sequence of what actually happened, separated from commentary.
4. **Ownership signal, if useful** — `GET /api/mportfolios` shows tracked managers' holdings.
   Interesting context; not an endorsement, and always lagged.
5. **State the gap explicitly** — a section listing what you could not obtain and why, with what
   the user could connect or paste to close it.
6. **Write it up** (`write-report`), usually into an `editor` document.

## Boundaries

No price target. No "undervalued". No buy/sell framing. You produce a structured description
plus measured price behaviour, and you name what is missing.

## Pitfalls

- **Filling the fundamentals hole from memory.** The failure this whole skill exists to prevent.
- **Wrong instrument.** Same ticker, different exchange, different company.
- **Sector as analysis.** "It is a semiconductor company" is identity, not insight.
- **Narrative from price.** A 30% fall is a fact; *why* it fell is a claim needing a source.

## Verification

Every figure traces to `findb`, `histdata`, or a dated news item. The "could not obtain" section
is present and specific — if it is empty, you either had a real data source or you invented
something; check which.

## Report shape

Identity block, price behaviour with its window, dated news timeline with sources, **data I could
not obtain**, and open questions. Length proportional to the evidence, not to the ambition.
