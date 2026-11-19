# Tax regimes

One directory per regime, one file per tax year, its test cases beside it:

```
regimes/<id>/<year>.toml         the rules
regimes/<id>/<year>.cases.toml   the cases that prove them
```

A country is data. The engine (`../assess.rs`, `../lots.rs`) runs the same steps for every
regime, and a file only sets their parameters. The schema is closed (`../regime.rs`,
`deny_unknown_fields`), so a rule the engine does not implement cannot be written down and
silently ignored.

## Adding a country or a year

1. Start from its sheet in the tax audit (`ProjectSpecs/audit/TAX_TOP30_*.md`).
2. Copy the closest existing regime, set `id`, `year`, `country`, `currency`, `sources`.
3. Express each rule with the blocks below. If a rule fits none, add a generic block to
   `regime.rs` + `assess.rs` with its own unit test, never a country branch.
4. Write the cases: at least two `[[case]]`, each naming its `source`; `[[lots_case]]` for
   the cost method or the anti-wash rule when the regime has one. Prefer worked examples
   from the official guidance.
5. Add the file to `regime::FILES`.
6. `cargo test -p otw-core taxcalc` runs every file and every case.
7. Leave `status = "draft"` until a person has checked every figure against the sources;
   then `status = "verified"` with `verified_on = "YYYY-MM-DD"`.

A new year of an existing regime is a copy of the previous file with the changed figures,
so an old return still computes under its own year's rules.

## Blocks

| Block | Meaning | Example |
| --- | --- | --- |
| `calendar` | First day of the tax year, and whether year N starts or ends in N | UK `04-06` start, AU `07-01` end |
| `fx_date` | Which day's rate converts a leg | `same_day`, `previous_day` (PL), `previous_month_end` (IN) |
| `matching.method` | Which purchase a sale is matched against | `average` (FR, IT), `fifo` (DE, US, ES), `uk_pool` |
| `matching.wash` | Loss deferred onto repurchases inside a window | US 30/30 days, ES 2/2 months |
| `pools` | Buckets that net together, carry years, UK allowance floor | FR securities 10 y, crypto 0 |
| `holding` | Holding tiers, giving items like `capital.long` | US, AU, DE crypto |
| `exempt` | Items not taxed at all | DE `crypto.long`, CH gains |
| `allowances` | `allowance` (subtracted) or `cliff` (all or nothing), on gain or proceeds | DE 1 000, FR crypto 305 on proceeds |
| `inclusion` | Share of the gain that is taxable | AU discount 0.5 |
| `schedules` | Brackets or the taxpayer's marginal rate, optionally stacked on other income | ES savings scale, US ordinary + LTCG |
| `surcharges` | A rate on the base (with a threshold) or on the tax | FR social charges, NIIT, Soli |

Items are the buckets `capital`, `derivative`, `crypto`, `dividends`, `interest`; a gain
bucket also has a term (`short` or a holding tier). `capital` selects every term,
`capital.long` one of them.
