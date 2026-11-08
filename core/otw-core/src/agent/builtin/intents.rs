//! Intent and slot table: what a request is about, what it needs, and what a silent message
//! defaults to.
//!
//! The prompt builder matches the user's message against [`INTENTS`] and, on a match, preloads
//! the intent's slots, its module's endpoint listing and its first skill, so the first tool
//! round goes to the task instead of to discovery. A miss preloads nothing: `load_skill` and
//! `otw_catalog` stay the way to the rest.

/// One kind of request.
pub struct Intent {
    pub name: &'static str,
    /// Lowercase. Matched on word boundaries; a trailing `*` matches any word starting with
    /// the rest (`optimi*` for optimize, optimise, optimization).
    pub keywords: &'static [&'static str],
    /// In preference order. The first one the agent's shelf allows is preloaded.
    pub skills: &'static [&'static str],
    /// The catalog module whose endpoints are preloaded.
    pub module: &'static str,
    /// Slots the task cannot run without. Asked for only when the message leaves one empty.
    pub required: &'static [&'static str],
    /// What a slot takes when the message says nothing about it.
    pub defaults: &'static [(&'static str, &'static str)],
}

pub const INTENTS: &[Intent] = &[
    Intent {
        name: "backtest",
        keywords: &[
            "backtest*", "back-test*", "back test*", "strategy", "strategies", "stop loss",
            "stop-loss", "take profit", "take-profit", "trailing stop", "out-of-sample",
            "out of sample", "in-sample", "in sample", "walk-forward", "optimi*", "overfit*",
            "sharpe", "profit factor", "max drawdown", "maximum drawdown", "cagr",
            "buy & hold", "buy and hold", "buy-and-hold", "crosses above", "crosses below",
            "crossover", "long position", "short position", "macd", "rsi", "sma", "ema",
            "bollinger", "stochastic", "supertrend", "atr", "adx", "donchian", "keltner",
        ],
        skills: &["backtest-run", "backtest-validate", "backtest-improve", "test-an-idea"],
        module: "backtest",
        required: &["instrument", "timeframe", "window (from, to)", "entry rule", "exit rule"],
        defaults: &[
            ("mode", "long"),
            ("fees", "fees {amount_kind: pct, per: trade, amount: 0.1} (0.1% per trade)"),
            ("slippage", "slippage {kind: pct, value: 0.0005} (a fraction: 0.05% per fill)"),
            ("sizing", "sizing {mode: percent_equity, percent: 100}"),
            (
                "oos_split_pct",
                "0.7 when validation or out-of-sample is asked (the in-sample head), else 0",
            ),
            ("view", "summary"),
        ],
    },
    Intent {
        name: "data",
        keywords: &[
            "download*", "historical data", "market data", "price data", "price history",
            "dataset*", "ohlc*", "candles", "bars", "intraday data", "tick data",
        ],
        skills: &["data-acquire"],
        module: "histdata",
        required: &["instrument", "timeframe", "window (from, to)"],
        defaults: &[
            ("asset_type", "equity for a stock ticker, crypto for a coin pair"),
            ("connector", "any ready connector that serves the asset type and timeframe"),
        ],
    },
    Intent {
        name: "journal-review",
        keywords: &[
            "journal", "my trades", "my trading", "trade log", "trading log", "losing streak",
            "winning streak", "session review", "review my session", "mistakes", "r-multiple*",
            "expectancy", "win rate", "winrate",
        ],
        skills: &["journal-audit", "session-review"],
        module: "journal",
        required: &["period"],
        defaults: &[("period", "every closed trade"), ("filters", "none")],
    },
    Intent {
        name: "portfolio-review",
        keywords: &[
            "portfolio*", "my book", "holdings", "allocation*", "exposure", "concentration",
            "rebalanc*", "diversif*", "stress test*", "correlation",
        ],
        skills: &["portfolio-review", "risk-metrics", "rebalance-plan", "stress-and-diversify"],
        module: "portfolios",
        required: &["portfolio"],
        defaults: &[("portfolio", "the only one when there is one, else ask which")],
    },
    Intent {
        name: "research",
        keywords: &[
            "research", "study", "company", "fundamental*", "earnings", "filing*", "10-k",
            "10-q", "valuation", "balance sheet", "income statement", "cash flow", "news",
            "headlines",
        ],
        skills: &["research-brief", "study-company", "source-check", "news-digest"],
        module: "fundamentals",
        required: &["subject"],
        defaults: &[("sources", "stored fundamentals and the news feeds")],
    },
    Intent {
        name: "scanner",
        keywords: &["scan", "scans", "scanner", "screen", "screener", "hive*", "watch for"],
        skills: &["scanner-setup"],
        module: "scanner",
        required: &["universe (items or a hive)", "signal"],
        defaults: &[],
    },
    Intent {
        name: "automator",
        keywords: &[
            "automat*", "workflow*", "every day at", "every morning", "every week",
            "on a schedule", "scheduled", "cron",
        ],
        skills: &[],
        module: "automator",
        required: &["trigger", "action"],
        defaults: &[],
    },
];

/// The intent with the most keywords present in `user_text` (case-insensitive). A tie goes to
/// the longer total match, then to table order. None when no keyword matches.
pub fn match_intent(user_text: &str) -> Option<&'static Intent> {
    let text = user_text.to_lowercase();
    let mut best: Option<(&'static Intent, usize, usize)> = None;
    for intent in INTENTS {
        let hits: Vec<&str> = intent.keywords.iter().copied().filter(|k| has_keyword(&text, k)).collect();
        if hits.is_empty() {
            continue;
        }
        let score = (hits.len(), hits.iter().map(|k| k.len()).sum::<usize>());
        if best.is_none_or(|(_, n, len)| score > (n, len)) {
            best = Some((intent, score.0, score.1));
        }
    }
    best.map(|(i, _, _)| i)
}

/// `keyword` in `text` on word boundaries. A trailing `*` drops the end boundary.
fn has_keyword(text: &str, keyword: &str) -> bool {
    let (word, prefix) = match keyword.strip_suffix('*') {
        Some(w) => (w, true),
        None => (keyword, false),
    };
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    text.match_indices(word).any(|(i, _)| {
        let before = text[..i].chars().next_back().is_none_or(|c| !is_word(c));
        let after = prefix || text[i + word.len()..].chars().next().is_none_or(|c| !is_word(c));
        before && after
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The stored run behind the robustness audit (1h variant), verbatim.
    const AUDIT_1H: &str = "Perform a complete backtest of a **Momentum trading strategy on AAPL \
        (Apple)** using the **1-hour timeframe**, based on the **MACD (12,26,9)** indicator. \
        Enter a long position when the MACD line crosses above the signal line and exit when it \
        crosses below. Implement a **strict maximum Stop Loss of 4% per trade**. Backtest 3–5 \
        years of historical data, including transaction fees and slippage. Report key \
        performance metrics: total return, CAGR, win rate, profit factor, maximum drawdown, \
        Sharpe ratio, number of trades, and comparison against Buy & Hold. Optimize parameters \
        while avoiding overfitting and validate the strategy using out-of-sample data.";

    /// The audit's other run: ten years of daily AAPL, MACD, 7% stop, out-of-sample and an
    /// optimization pass.
    const AUDIT_10Y: &str = "Backtest a MACD (12,26,9) strategy on AAPL over the last 10 years \
        on the daily timeframe: go long when the MACD line crosses above the signal line, exit \
        when it crosses below, with a 7% stop loss. Include fees and slippage, report CAGR, win \
        rate, profit factor, max drawdown and Sharpe against buy and hold, then optimize the \
        parameters and validate out of sample.";

    #[test]
    fn audit_prompts_match_backtest() {
        for p in [AUDIT_1H, AUDIT_10Y] {
            let i = match_intent(p).expect("an intent");
            assert_eq!(i.name, "backtest");
            assert_eq!(i.skills.first(), Some(&"backtest-run"));
        }
    }

    #[test]
    fn other_intents_match() {
        let cases = [
            ("Download 5 years of BTCUSDT 1h candles", "data"),
            ("Review my journal for last month", "journal-review"),
            ("How concentrated is my portfolio? Should I rebalance?", "portfolio-review"),
            ("Study the company and its latest earnings", "research"),
            ("Set up a screener on my hive", "scanner"),
            ("Build a workflow that runs every morning", "automator"),
        ];
        for (text, want) in cases {
            assert_eq!(match_intent(text).map(|i| i.name), Some(want), "{text}");
        }
    }

    #[test]
    fn no_keyword_no_intent() {
        assert!(match_intent("hello, what can you do?").is_none());
        assert!(match_intent("").is_none());
    }

    /// Short keywords need word boundaries: "sma" is not in "small", "ema" not in "email".
    #[test]
    fn keywords_respect_word_boundaries() {
        assert!(!has_keyword("a small email", "sma"));
        assert!(!has_keyword("a small email", "ema"));
        assert!(has_keyword("sma(20) over ema", "sma"));
        assert!(has_keyword("optimization", "optimi*"));
        assert!(!has_keyword("reoptimize", "optimi*"));
        assert!(has_keyword("BUY & HOLD".to_lowercase().as_str(), "buy & hold"));
    }

    #[test]
    fn match_is_case_insensitive() {
        assert_eq!(match_intent("BACKTEST MACD ON AAPL").map(|i| i.name), Some("backtest"));
    }

    /// Every skill an intent names must ship, or the preload silently loads nothing.
    #[test]
    fn intent_skills_exist() {
        for i in INTENTS {
            for s in i.skills {
                assert!(super::super::SKILLS.iter().any(|b| &b.name == s), "{}: {s}", i.name);
            }
        }
    }

    /// Every intent module must be a catalog module.
    #[test]
    fn intent_modules_exist() {
        for i in INTENTS {
            assert!(
                crate::mcp::catalog::MODULES.iter().any(|(m, _)| *m == i.module),
                "{}: unknown module {}",
                i.name,
                i.module
            );
        }
    }

    #[test]
    fn keywords_are_lowercase() {
        for i in INTENTS {
            for k in i.keywords {
                assert_eq!(*k, k.to_lowercase(), "{}: {k}", i.name);
            }
        }
    }
}
