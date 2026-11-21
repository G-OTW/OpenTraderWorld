-- Chart alerts: `channels` used `[]` for "every channel the chart is granted", so an alert
-- could not be kept in the app. "All" is now the explicit string `"all"` and `[]` means no
-- channel. Existing alerts keep notifying where they did.
UPDATE histviz_alerts SET channels = '"all"'::jsonb WHERE channels = '[]'::jsonb;
ALTER TABLE histviz_alerts ALTER COLUMN channels SET DEFAULT '"all"'::jsonb;
