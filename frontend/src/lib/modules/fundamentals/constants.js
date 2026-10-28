/**
 * Fixed reference lists of the Fundamentals pages. Ids match the backend (`CATEGORIES` in
 * `fundamentals/mod.rs`, the statement line keys EDGAR stores, the capability families);
 * labels come from i18n.
 */

/** Macro categories, in display order. */
export const MACRO_CATEGORIES = ['growth', 'inflation', 'labour', 'rates', 'money', 'surveys', 'housing', 'energy', 'fiscal', 'positioning', 'other'];

/**
 * US recessions as dated by the NBER Business Cycle Dating Committee, peak month to trough
 * month (the same periods FRED's `USREC` flags). Add a row when the committee announces one.
 */
const NBER = [
  ['1948-11', '1949-10'],
  ['1953-07', '1954-05'],
  ['1957-08', '1958-04'],
  ['1960-04', '1961-02'],
  ['1969-12', '1970-11'],
  ['1973-11', '1975-03'],
  ['1980-01', '1980-07'],
  ['1981-07', '1982-11'],
  ['1990-07', '1991-03'],
  ['2001-03', '2001-11'],
  ['2007-12', '2009-06'],
  ['2020-02', '2020-04']
];
const monthMs = (ym) => Date.UTC(+ym.slice(0, 4), +ym.slice(5, 7) - 1, 1);
export const RECESSIONS = NBER.map(([a, b]) => [monthMs(a), monthMs(b)]);

/** Statement lines in display order, as the backend stores them. */
export const STATEMENT_LINES = {
  income: ['revenue', 'cost_of_revenue', 'gross_profit', 'rnd', 'sga', 'operating_income', 'interest_expense', 'pretax_income', 'income_tax', 'net_income', 'ebitda', 'eps_diluted'],
  balance: ['cash', 'short_term_investments', 'receivables', 'inventory', 'total_current_assets', 'ppe_net', 'goodwill_intangibles', 'total_assets', 'accounts_payable', 'short_term_debt', 'total_current_liabilities', 'long_term_debt', 'total_liabilities', 'shareholders_equity'],
  cashflow: ['net_income', 'depreciation', 'stock_comp', 'operating_cash_flow', 'capex', 'acquisitions', 'investing_cash_flow', 'dividends_paid', 'buybacks', 'financing_cash_flow', 'free_cash_flow']
};

/** How many recently opened symbols a picker shows after the favourites. */
export const RECENT_SYMBOLS = 15;

/** Quick picks on the ETF page until an ETF has been opened; any ticker can be typed. */
export const SUGGESTED_ETFS = ['SPY', 'QQQ', 'IWM', 'EFA', 'VGK', 'EEM', 'TLT', 'GLD'];

/** Data families of the coverage matrix (columns), as connectors declare them. */
export const DATA_ELEMENTS = ['macro', 'cot', 'statements', 'filings', 'insiders', 'estimates', 'earnings', 'segments', 'dividends', 'holders', 'short_interest', 'peers', 'esg', 'etf', 'calendar', 'transcripts', 'alt'];
