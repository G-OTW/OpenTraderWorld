# Import fixtures

Synthetic broker exports, written by hand from each format's documented or observed layout.
No real account, name, number or trade: tickers are public symbols, prices and quantities are
invented, account ids are placeholders (`U0000000`).

- `generic/`: trade books in the shapes the journal import reads (one row per trade, one row
  per fill, a French semicolon export saved in Latin-1).
- `portfolio/`: an operations ledger for the portfolio import.
- `ibkr/`: an Activity Flex Query statement (XML), as the Flex Web Service returns it.

Corrupted variants (no header, BOM, other delimiter, truncated line, UTF-16) are derived from
these files inside the tests, so each corruption stays visible next to its assertion.
