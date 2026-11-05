-- Form 4 filings already parsed for a company, including the ones that yield no trade in
-- its own shares (derivatives only, or filed by the company as the reporting owner of
-- another issuer's stock), so a refresh does not fetch them again.
CREATE TABLE IF NOT EXISTS fund_form4_seen (
    company_id UUID NOT NULL REFERENCES fund_companies(id) ON DELETE CASCADE,
    accession  TEXT NOT NULL,
    PRIMARY KEY (company_id, accession)
);
