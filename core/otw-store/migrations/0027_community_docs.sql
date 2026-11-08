-- Community Docs module.
--
-- A library of community-authored documents shown in-app and kept available offline.
-- Each doc has a slug (stable id from the source), title, summary, category, and an
-- HTML body. Docs originate from the website and are synced daily or manually; the
-- `source_url` and `synced_at` columns track provenance. Single-user: no owner scoping.
--
-- Body is trusted HTML (curated/sanitized at the source). The frontend renders it
-- directly, so never store unsanitized user input here.

CREATE TABLE IF NOT EXISTS community_docs (
    id         UUID PRIMARY KEY,
    slug       TEXT NOT NULL UNIQUE,
    title      TEXT NOT NULL DEFAULT '',
    summary    TEXT NOT NULL DEFAULT '',
    category   TEXT NOT NULL DEFAULT '',
    body       TEXT NOT NULL DEFAULT '',
    source_url TEXT NOT NULL DEFAULT '',
    synced_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_community_docs_category ON community_docs(category);

-- Seed two starter docs (placeholders until the website sync lands).
INSERT INTO community_docs (id, slug, title, summary, category, body) VALUES
(
    gen_random_uuid(),
    'risk-percentage-of-stack',
    'Risk a percentage of your stack, not the whole stack',
    'Why sizing each trade as a small percentage of your capital keeps you in the game.',
    'Risk Management',
    $html$
<p>The single fastest way to blow up an account is to bet too much on one idea.
Professional traders almost never risk their <em>full stack</em> on a trade. Instead
they risk a small, fixed <strong>percentage</strong> of their capital on each position.</p>

<h2>The core rule</h2>
<p>Decide up front how much of your account you are willing to lose on a single trade.
A common figure is <strong>1%</strong> (conservative) to <strong>2%</strong> (aggressive)
of your total stack. This is your <em>risk per trade</em> — not the size of the position,
but the amount you lose if your stop is hit.</p>

<h2>How to size a position</h2>
<ol>
  <li><strong>Pick your risk %.</strong> Say 1% of a $10,000 account = <strong>$100</strong>.</li>
  <li><strong>Set your stop.</strong> Decide where the trade is wrong, e.g. entry $50, stop $48 → $2 of risk per share/unit.</li>
  <li><strong>Divide.</strong> Position size = risk &divide; per-unit risk = $100 &divide; $2 = <strong>50 units</strong>.</li>
</ol>
<p>Now the worst case is a known, survivable loss — regardless of how confident you feel.</p>

<h2>Why percentage beats full-stack</h2>
<ul>
  <li><strong>You survive losing streaks.</strong> Risking 1% means even 10 losses in a row only costs ~10% of capital. Risking the full stack means <em>one</em> loss ends you.</li>
  <li><strong>Position size scales automatically.</strong> As your account grows, 1% grows with it; as it shrinks, you naturally bet less.</li>
  <li><strong>It removes emotion.</strong> The math sets the size, not your gut.</li>
</ul>

<h2>The drawdown reality</h2>
<p>Losses compound against you. The deeper the hole, the harder it is to climb out:</p>
<table>
  <thead><tr><th>Loss taken</th><th>Gain needed to recover</th></tr></thead>
  <tbody>
    <tr><td>10%</td><td>11%</td></tr>
    <tr><td>25%</td><td>33%</td></tr>
    <tr><td>50%</td><td>100%</td></tr>
    <tr><td>90%</td><td>900%</td></tr>
  </tbody>
</table>
<p>Risking small keeps you on the gentle left side of this table.</p>

<p><strong>Bottom line:</strong> never ask &ldquo;how much can I make?&rdquo; before asking
&ldquo;how much can I lose?&rdquo; Cap that loss at a small percentage and let the winners
take care of themselves.</p>
$html$
),
(
    gen_random_uuid(),
    'candlestick-patterns-intro',
    'Reading candlestick patterns',
    'A quick visual guide to how candles are built and a few common reversal patterns.',
    'Technical Analysis',
    $html$
<p>A <strong>candlestick</strong> packs four prices — open, high, low, close — into one
shape. The thick part is the <em>body</em> (open-to-close); the thin lines are the
<em>wicks</em> (or shadows), reaching to the high and low.</p>

<h2>Anatomy of a candle</h2>
<svg viewBox="0 0 320 200" width="100%" style="max-width:360px" role="img" aria-label="Bullish and bearish candle anatomy">
  <style>
    .up{fill:#1bbf83;stroke:#1bbf83}
    .down{fill:#e15241;stroke:#e15241}
    .lbl{fill:currentColor;font:11px sans-serif}
  </style>
  <!-- bullish -->
  <line x1="90" y1="20" x2="90" y2="180" class="up" stroke-width="2"/>
  <rect x="74" y="70" width="32" height="70" class="up"/>
  <text x="118" y="35" class="lbl">High</text>
  <text x="118" y="74" class="lbl">Close</text>
  <text x="118" y="144" class="lbl">Open</text>
  <text x="118" y="180" class="lbl">Low</text>
  <text x="62" y="196" class="lbl">Bullish</text>
  <!-- bearish -->
  <line x1="230" y1="20" x2="230" y2="180" class="down" stroke-width="2"/>
  <rect x="214" y="60" width="32" height="70" class="down"/>
  <text x="200" y="196" class="lbl">Bearish</text>
</svg>
<p>A <strong>green/bullish</strong> candle closes above its open (buyers won the session);
a <strong>red/bearish</strong> candle closes below its open (sellers won).</p>

<h2>A few common patterns</h2>
<h3>Doji — indecision</h3>
<p>Open and close are nearly equal, so the body is tiny. Neither side took control;
often a warning that the current trend is stalling.</p>

<h3>Hammer — potential bullish reversal</h3>
<p>A small body near the top with a long lower wick, appearing after a downtrend. Sellers
pushed price down but buyers slammed it back up by the close.</p>

<h3>Shooting star — potential bearish reversal</h3>
<p>The mirror of the hammer: small body near the bottom with a long upper wick, after an
uptrend. Buyers pushed up but sellers rejected the highs.</p>

<h3>Engulfing — momentum shift</h3>
<p>A candle whose body completely &ldquo;engulfs&rdquo; the previous one. A bullish
engulfing (big green after a red) hints at a turn up; a bearish engulfing hints at a turn down.</p>

<h2>How to use them</h2>
<ul>
  <li><strong>Context matters.</strong> A hammer mid-trend means little; at support after a drop it means more.</li>
  <li><strong>Confirm.</strong> Wait for the next candle or another signal before acting.</li>
  <li><strong>Combine, don&rsquo;t isolate.</strong> Patterns work best alongside trend, volume and key levels.</li>
</ul>
<p>Candlesticks describe <em>who is winning the fight</em> between buyers and sellers —
read them as a story, not a crystal ball.</p>
$html$
)
ON CONFLICT (slug) DO NOTHING;
