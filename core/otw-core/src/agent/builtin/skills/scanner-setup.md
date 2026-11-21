# Setting up a scan

## When to use

The user wants the Scanner to watch a set of instruments for a condition and notify them.
A scan only notifies: it never places an order. Call `otw_catalog {endpoint}` before each
body below and use its field names exactly.

## Procedure

1. **Items.** One item is one instrument on a data connector at one timeframe. Find the
   symbol with `GET /api/histdata/symbols?module=scanner` and copy `provider`, `asset_type`,
   `ticker` and the `timeframe` as returned: never a guessed symbol. `POST /api/scanner/items`
   per instrument. `GET /api/scanner/items` first: an item that already exists is a 409.
2. **Hive.** `POST /api/scanner/hives` with a `name`. Then `PUT /api/scanner/hives/{id}/items`
   with `item_ids`: the WHOLE list, it replaces the current members, so to add to a hive send
   the ids it already holds plus the new ones. The cap is in `GET /api/scanner/hives`.
3. **Signal.** Pick the builder that says what the user means: `pattern` (pin_bar,
   inside_bar, engulfing...), `event` (move_from_open, gap, volume_spike...), `signal` (a
   backtest condition group over indicators) or `time` (a session or hour window). `pct` is
   a percent (1.5 = 1.5%). Test the draft with `POST /api/scanner/signals/test` on one item
   that has stored bars, then `POST /api/scanner/signals`.
4. **Check it.** `held_count` 0 over hundreds of bars is a suspect threshold, and a signal
   that holds on most bars is not a filter. Say which before going on.
5. **Alert.** `POST /api/scanner/alerts` with `target_kind` hive, the hive id and the
   `signal_ids` (all must hold on the same closed bar; OR is two alerts). Then
   `POST /api/scanner/alerts/{id}/run-now`: a preview over the whole hive that records no hit
   and notifies nobody. Report `hits` and each row's `error`.

```jsonc
// POST /api/scanner/signals
{ "name": "Bullish pin bar", "builder": "pattern",
  "config": { "kind": "pin_bar", "direction": "up" } }
// POST /api/scanner/alerts
{ "target_kind": "hive", "target_id": "<hive id>", "signal_ids": ["<signal id>"],
  "notify": "digest", "cooldown_secs": 3600 }
```

## Pitfalls

- A signal test reads stored bars only. An item with no dataset is a 400: download it
  (data-acquire) or rely on run-now, which fetches through the item's connector.
- Deleting items, signals and alerts is the user's to do in the Scanner page.
