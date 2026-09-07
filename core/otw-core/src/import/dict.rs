//! The import vocabulary: what a field can be called, in the languages the app ships,
//! plus the header conventions brokers actually use.
//!
//! This is data, not logic — extending it is adding a string. The user's own
//! corrections are learned into `import_aliases` and win over everything here.
//!
//! The synonym lists are shared; what differs per module is which **targets** it offers.
//! A journal imports trades (entry/exit, leverage, strategy); a portfolio imports an
//! operations ledger (one date, one price, one side). Offering a module a target it has
//! nowhere to put would only invite a wrong mapping, so each declares its own set.

/// What kind of value a target field accepts. Used to reject a header match whose
/// column obviously holds something else (a "Price" column full of words).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Text,
    Num,
    Date,
    Side,
    Currency,
    Enum,
}

pub struct Target {
    pub id: &'static str,
    pub kind: Kind,
    pub synonyms: &'static [&'static str],
}

/// The targets one module offers. `field:<name>` (custom) and `ignore` are handled
/// outside these tables.
pub struct TargetSet(pub &'static [Target]);

impl TargetSet {
    pub fn iter(&self) -> impl Iterator<Item = &'static Target> {
        self.0.iter()
    }
    pub fn get(&self, id: &str) -> Option<&'static Target> {
        self.0.iter().find(|t| t.id == id)
    }
    pub fn has(&self, id: &str) -> bool {
        self.get(id).is_some()
    }
    /// Kind of a mapping target, including the pseudo-targets.
    pub fn kind(&self, id: &str) -> Kind {
        if id.starts_with("field:") {
            return Kind::Text;
        }
        self.get(id).map(|t| t.kind).unwrap_or(Kind::Text)
    }
    /// The ids of every target of a given kind, in declaration order — the order the
    /// value-only inference proposes them in when a header said nothing.
    pub fn of_kind(&self, kind: Kind) -> impl Iterator<Item = &'static str> + use<'_> {
        self.0.iter().filter(move |t| t.kind == kind).map(|t| t.id)
    }
}

// ── Synonyms ─────────────────────────────────────────────────────────────────

const TICKER: &[&str] = &[
    "ticker", "symbol", "symbole", "instrument", "pair", "paire", "product", "produit",
    "security", "titre", "valeur", "underlying", "sous jacent", "coin", "asset name",
    "simbolo", "activo", "instrumento", "wertpapier", "basiswert", "strumento",
    "ativo", "codigo", "epic", "contract", "contrat", "stock code", "share",
];

const ASSET_CLASS: &[&str] = &[
    "asset class", "asset type", "instrument type", "security type", "classe d actif",
    "type d actif", "type d instrument", "clase de activo", "tipo de activo",
    "anlageklasse", "instrumententyp", "tipo di strumento", "classe di attivita",
    "classe de ativo", "tipo de ativo", "asset category", "market type",
];

const EXCHANGE: &[&str] = &[
    "exchange", "venue", "market", "marche", "bourse", "place", "broker", "courtier",
    "mercado", "bolsa", "borse", "handelsplatz", "borsa", "mercato", "listing exchange",
    "mic", "platform", "plateforme",
];

const SIDE: &[&str] = &[
    "side", "direction", "action", "buy sell", "buy or sell", "b s", "long short",
    "position", "sens", "achat vente", "trade type", "order type", "type of trade",
    "operation", "operacion", "tipo de operacion", "compra venta", "kauf verkauf",
    "richtung", "operazione", "compra venda", "way", "l s", "transaction type",
];

const CURRENCY: &[&str] = &[
    "currency", "ccy", "devise", "monnaie", "moneda", "divisa", "wahrung", "valuta",
    "moeda", "quote currency", "account currency", "settlement currency", "cur",
];

const UNIT_TYPE: &[&str] = &[
    "unit type", "unit", "units", "unite", "type d unite", "tipo de unidad", "einheit",
    "unita", "unidade",
];

/// The date a row happened. Named "entry" in a trade book, plain "date" in a ledger —
/// the same words serve both.
const DATE: &[&str] = &[
    "entry time", "entry date", "entry", "open time", "open date", "opened", "opening time",
    "date opened", "buy date", "trade date", "transaction date", "execution time",
    "fill time", "date time", "datetime", "timestamp", "date", "time", "open",
    "date d entree", "entree", "date d ouverture", "ouverture", "heure d entree",
    "fecha de entrada", "entrada", "apertura", "fecha", "hora",
    "einstieg", "eroffnung", "kaufdatum", "datum", "zeit",
    "data di ingresso", "ingresso", "data", "ora",
    "data de entrada", "abertura", "executed at", "created at",
];

const EXIT_AT: &[&str] = &[
    "exit time", "exit date", "exit", "close time", "close date", "closed", "closing time",
    "date closed", "sell date", "close", "date de sortie", "sortie", "fermeture",
    "cloture", "heure de sortie", "fecha de salida", "salida", "cierre", "ausstieg",
    "schliessung", "verkaufsdatum", "data di uscita", "uscita", "chiusura",
    "data de saida", "encerramento", "closed at",
];

/// The price a row was filled at.
const PRICE: &[&str] = &[
    "entry price", "open price", "opening price", "buy price", "price open", "avg entry",
    "average entry", "entry avg", "avg entry price", "fill price", "execution price",
    "exec price", "avg price", "average price", "price", "rate", "open rate", "price in",
    "prix d entree", "prix d achat", "cours d entree", "prix", "cours",
    "precio de entrada", "precio de compra", "precio", "einstiegskurs", "kaufpreis",
    "preis", "kurs", "prezzo di ingresso", "prezzo", "preco de entrada", "preco",
];

const EXIT_PRICE: &[&str] = &[
    "exit price", "close price", "closing price", "sell price", "price close", "avg exit",
    "average exit", "avg exit price", "close rate", "price out", "prix de sortie",
    "prix de vente", "cours de sortie", "precio de salida", "precio de venta",
    "ausstiegskurs", "verkaufspreis", "prezzo di uscita", "preco de saida",
];

const QUANTITY: &[&str] = &[
    "quantity", "qty", "size", "volume", "shares", "units", "contracts", "lots", "lot",
    "position size", "filled qty", "executed qty", "exec qty", "no of shares", "nb",
    "quantite", "taille", "actions", "parts", "contrats", "nombre", "cantidad", "tamano",
    "acciones", "menge", "anzahl", "stuck", "grosse", "quantita", "dimensione", "azioni",
    "quantidade", "tamanho", "filled", "shares qty",
];

const FEES: &[&str] = &[
    "fee", "fees", "commission", "commissions", "charges", "brokerage", "total fees",
    "transaction fee", "trading fee", "ibcommission", "comm", "frais", "frais de courtage",
    "couts", "comision", "comisiones", "gastos", "gebuhr", "gebuhren", "provision",
    "kosten", "commissioni", "spese", "costi", "taxas", "comissao", "custos",
];

const LEVERAGE: &[&str] =
    &["leverage", "levier", "apalancamiento", "hebel", "leva", "alavancagem", "lev"];

const MULTIPLIER: &[&str] = &[
    "multiplier", "contract size", "multiplicateur", "taille du contrat", "multiplicador",
    "multiplikator", "kontraktgrosse", "moltiplicatore", "point value", "lot size",
];

const SIGNAL_NAME: &[&str] = &[
    "signal", "setup", "pattern", "signal name", "nom du signal", "senal", "configuracion",
    "muster", "aufbau", "segnale", "sinal", "trigger",
];

const NOTE: &[&str] = &[
    "notes", "note", "comment", "comments", "remarks", "feedback", "description",
    "commentaire", "commentaires", "remarques", "notas", "comentarios", "observaciones",
    "anmerkung", "notiz", "kommentar", "bemerkungen", "commenti", "observacoes",
    "comentarios", "trade notes", "review", "journal", "memo",
];

const STRATEGY: &[&str] = &[
    "strategy", "strategie", "estrategia", "strategia", "estrategia", "system", "systeme",
    "playbook", "method", "methode",
];

const CATEGORY: &[&str] = &[
    "category", "account", "portfolio", "book", "compte", "portefeuille", "categorie",
    "cuenta", "cartera", "categoria", "konto", "kategorie", "conto", "portafoglio",
    "conta", "carteira", "account name", "account id", "sub account",
];

const EXTERNAL_ID: &[&str] = &[
    "order id", "trade id", "transaction id", "deal id", "position id", "ticket",
    "reference", "ref", "id", "identifiant", "numero d ordre", "reference",
    "id de orden", "identificador", "auftragsnummer", "referenz", "id ordine",
    "riferimento", "order ref", "confirmation", "orderid", "tradeid",
];

const PNL_CHECK: &[&str] = &[
    "pnl", "p l", "p and l", "profit", "profit loss", "net pnl", "gross pnl",
    "realized pnl", "realized p l", "net profit", "result", "gain", "gain loss",
    "resultat", "gain perte", "benefice", "profit net", "resultado", "ganancia",
    "beneficio", "gewinn", "verlust", "ergebnis", "risultato", "utile", "lucro",
    "return", "realized",
];

/// The cash the row moved. A ledger export often states the total and leaves the unit
/// price implicit — with a quantity beside it, one gives the other.
const AMOUNT: &[&str] = &[
    "amount", "gross amount", "net amount", "total amount", "total", "value", "proceeds",
    "consideration", "cost", "turnover", "montant", "montant brut", "montant net",
    "valeur", "importe", "total bruto", "importo", "controvalore", "betrag", "gesamt",
    "wert", "valor", "valor total", "subtotal", "net", "gross",
];

// ── Target sets ──────────────────────────────────────────────────────────────

/// Every column a trade book can be mapped onto.
pub static JOURNAL: TargetSet = TargetSet(&[
    Target { id: "ticker", kind: Kind::Text, synonyms: TICKER },
    Target { id: "asset_class", kind: Kind::Enum, synonyms: ASSET_CLASS },
    Target { id: "exchange", kind: Kind::Text, synonyms: EXCHANGE },
    Target { id: "side", kind: Kind::Side, synonyms: SIDE },
    Target { id: "currency", kind: Kind::Currency, synonyms: CURRENCY },
    Target { id: "unit_type", kind: Kind::Enum, synonyms: UNIT_TYPE },
    Target { id: "entry_at", kind: Kind::Date, synonyms: DATE },
    Target { id: "exit_at", kind: Kind::Date, synonyms: EXIT_AT },
    Target { id: "entry_price", kind: Kind::Num, synonyms: PRICE },
    Target { id: "exit_price", kind: Kind::Num, synonyms: EXIT_PRICE },
    Target { id: "quantity", kind: Kind::Num, synonyms: QUANTITY },
    Target { id: "fees", kind: Kind::Num, synonyms: FEES },
    Target { id: "leverage", kind: Kind::Num, synonyms: LEVERAGE },
    Target { id: "multiplier", kind: Kind::Num, synonyms: MULTIPLIER },
    Target { id: "signal_name", kind: Kind::Text, synonyms: SIGNAL_NAME },
    Target { id: "feedback", kind: Kind::Text, synonyms: NOTE },
    Target { id: "strategy", kind: Kind::Text, synonyms: STRATEGY },
    Target { id: "category", kind: Kind::Text, synonyms: CATEGORY },
    Target { id: "external_id", kind: Kind::Text, synonyms: EXTERNAL_ID },
    Target { id: "pnl_check", kind: Kind::Num, synonyms: PNL_CHECK },
]);

/// Every column a portfolio's operations ledger can be mapped onto. One row is one
/// buy or one sell: a single date, a single price, no exit side of its own.
pub static PORTFOLIO: TargetSet = TargetSet(&[
    Target { id: "ticker", kind: Kind::Text, synonyms: TICKER },
    Target { id: "side", kind: Kind::Side, synonyms: SIDE },
    Target { id: "op_date", kind: Kind::Date, synonyms: DATE },
    Target { id: "quantity", kind: Kind::Num, synonyms: QUANTITY },
    Target { id: "price", kind: Kind::Num, synonyms: PRICE },
    Target { id: "amount", kind: Kind::Num, synonyms: AMOUNT },
    Target { id: "fee", kind: Kind::Num, synonyms: FEES },
    Target { id: "currency", kind: Kind::Currency, synonyms: CURRENCY },
    Target { id: "note", kind: Kind::Text, synonyms: NOTE },
    Target { id: "external_id", kind: Kind::Text, synonyms: EXTERNAL_ID },
]);

// ── Value vocabularies ───────────────────────────────────────────────────────

/// Words that mean "opening / long" and "closing / short" in a side column. Matched on
/// the normalized cell value. Extended per-mapping by `value_maps.side`.
pub const SIDE_LONG: &[&str] = &[
    "long", "l", "buy", "b", "bought", "bot", "buy to open", "buy to close", "achat", "acheter",
    "acheté", "achete", "compra", "comprar", "kauf", "kaufen", "acquisto", "acquistare",
    "compra venda", "1", "+1", "entry long", "open long", "bid", "in",
];
pub const SIDE_SHORT: &[&str] = &[
    "short", "s", "sell", "sld", "sold", "sell to open", "sell to close", "vente", "vendre",
    "vendu", "venta", "vender", "verkauf", "verkaufen", "vendita", "vendere", "venda",
    "-1", "2", "entry short", "open short", "ask", "out",
];

/// Words that name a ledger kind other than a trade.
///
/// A broker statement writes these in the *same* column as its buys and sells, so they are
/// read there before the direction vocabulary: "Dividend" is not a direction, and reading it
/// as one is exactly how a statement's income lines used to become row errors.
pub const KIND_MAP: &[(&str, &[&str])] = &[
    (
        "dividend",
        &[
            "dividend", "dividends", "div", "cash dividend", "qualified dividend",
            "reinvested dividend", "dividende", "dividendes", "dividendo", "dividendos",
            "dividenden", "dividendi", "dividendenzahlung",
        ],
    ),
    (
        "interest",
        &[
            "interest", "interests", "interest income", "credit interest", "interest paid",
            "interet", "interets", "interes", "intereses", "zins", "zinsen", "interesse",
            "interessi", "juros",
        ],
    ),
    (
        "coupon",
        &["coupon", "coupons", "bond coupon", "cupon", "cupones", "kupon", "cedola", "cedole"],
    ),
    (
        "deposit",
        &[
            "deposit", "deposits", "funding", "cash in", "transfer in", "wire in",
            "depot", "depots", "deposito", "depositos", "einzahlung", "versamento",
        ],
    ),
    (
        "withdraw",
        &[
            "withdraw", "withdrawal", "withdrawals", "cash out", "transfer out", "wire out",
            "retrait", "retraits", "retiro", "retiros", "auszahlung", "prelievo", "levantamento",
        ],
    ),
    (
        "fee",
        &[
            "fee", "fees", "commission", "commissions", "custody fee", "management fee",
            "account fee", "frais", "comision", "comisiones", "gebuhr", "gebuhren",
            "commissione", "commissioni", "taxa",
        ],
    ),
    (
        "tax",
        &[
            "tax", "taxes", "withholding", "withholding tax", "impot", "impots", "impuesto",
            "impuestos", "steuer", "steuern", "imposta", "imposte", "imposto",
        ],
    ),
];

/// Source words for the journal's asset classes.
pub const ASSET_MAP: &[(&str, &[&str])] = &[
    (
        "stock",
        &[
            "stock", "stocks", "equity", "equities", "share", "shares", "action", "actions",
            "aktie", "aktien", "azione", "azioni", "accion", "acciones", "acao", "acoes", "stk",
        ],
    ),
    ("etf", &["etf", "etfs", "fund", "fonds", "fondo", "tracker"]),
    (
        "crypto",
        &["crypto", "cryptos", "cryptocurrency", "coin", "token", "digital asset", "kryptowahrung"],
    ),
    (
        "forex",
        &["forex", "fx", "currency", "currencies", "devise", "devises", "cash", "divisa", "waehrung"],
    ),
    (
        "future",
        &["future", "futures", "fut", "contract for difference", "cfd", "contrat", "futuro", "termine"],
    ),
    ("option", &["option", "options", "opt", "warrant", "opzione", "opcion", "optionsschein"]),
];

/// Source words for the journal's unit types.
pub const UNIT_MAP: &[(&str, &[&str])] = &[
    ("share", &["share", "shares", "action", "actions", "aktie", "azione", "accion", "acao", "stk"]),
    ("contract", &["contract", "contracts", "contrat", "contrats", "kontrakt", "contratto", "cfd"]),
    ("lot", &["lot", "lots", "los", "lotto", "lote"]),
    ("unit", &["unit", "units", "unite", "unites", "einheit", "unita", "unidade", "coin", "token"]),
];

/// ISO-4217 codes the journal stores. Anything else falls back to the mapping default.
pub const CURRENCIES: &[&str] = &[
    "USD", "EUR", "GBP", "JPY", "CNY", "CHF", "CAD", "AUD", "HKD", "SEK", "NOK", "DKK",
];

/// Short month names the date parser understands (en/fr/es/de/it/pt, accents stripped).
pub const MONTHS: &[(&str, u8)] = &[
    ("jan", 1), ("janv", 1), ("ene", 1), ("gen", 1),
    ("feb", 2), ("fev", 2), ("fevr", 2), ("febr", 2),
    ("mar", 3), ("mars", 3), ("marz", 3), ("mrz", 3),
    ("apr", 4), ("avr", 4), ("abr", 4),
    ("may", 5), ("mai", 5), ("mag", 5), ("maio", 5),
    ("jun", 6), ("juin", 6), ("giu", 6),
    ("jul", 7), ("juil", 7), ("lug", 7),
    ("aug", 8), ("aou", 8), ("ago", 8),
    ("sep", 9), ("sept", 9), ("set", 9),
    ("oct", 10), ("okt", 10), ("ott", 10), ("out", 10),
    ("nov", 11),
    ("dec", 12), ("dez", 12), ("dic", 12),
];
