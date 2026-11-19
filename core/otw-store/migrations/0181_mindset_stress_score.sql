-- The seeded "Stress" scale runs Calm to Wired, so its high end is the bad one. A scale with
-- no `config.score` counts higher as better, which raised the daily mindset score with stress.
-- Only the untouched seeded prompt is set; a direction the user already chose stays.
UPDATE mindset_prompts
SET config = config || '{"score":"desc"}'::jsonb
WHERE kind = 'scale'
  AND label = 'Stress'
  AND config->>'low' = 'Calm'
  AND config->>'high' = 'Wired'
  AND NOT config ? 'score';
