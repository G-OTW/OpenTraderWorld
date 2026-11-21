# End-to-end seed

The deterministic data set an end-to-end run (Playwright, testbook) starts from: the same
trades, portfolios, watchlists and price history on every run, so a screen can be compared
to a known state.

Rules on top of the root README:

- Every value is fixed: no `now()`, no random id without a fixed seed, dates written out.
- The seed is loaded through the application's own API or import, not by writing tables
  directly, so it stays valid when the schema moves.
- Prefer reusing the demo data set (`core/otw-core/src/demo_seed.rs`) to inventing a second
  one; a file here states which part of it it extends and why.
