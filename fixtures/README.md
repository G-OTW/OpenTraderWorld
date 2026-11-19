# Test fixtures

Frozen inputs shared by the test suites (Rust crates, frontend, testbook). A test reads them
from disk and never downloads them.

- `market/`: price series (CSV), for indicators, backtests and benchmarks.
- `imports/`: broker and trade-book exports, one folder per broker or format.
- `api/`: recorded or hand-written responses of external providers (JSON).
- `seed/`: the deterministic data set an end-to-end run starts from.

Rules for every file here:

- **No personal or real account data.** No name, e-mail, account number, API key, token or
  real trade of a real person. Account ids are placeholders, amounts are invented or come from
  public market data.
- **Provenance is written down.** Each folder's README says, per file, where it comes from:
  generated (with the generator and its seed, so it can be rebuilt bit for bit), written by
  hand (from which documented format), or public market data (source, date of capture, licence).
- **Small and readable in review.** A large file needs a line in the README saying why a
  smaller one would not do.
- **Frozen.** A fixture is never edited to make a test pass. A new case is a new file, or a
  corruption derived inside the test so it stays visible next to its assertion.
