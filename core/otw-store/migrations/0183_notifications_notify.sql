-- Every new notification is announced on the `otw_notifications` channel, so the open app
-- shows its toast as soon as the row exists instead of at the next poll. Rows are inserted
-- from many places (reminders, chart, watchlist and scanner alerts, automator, paper, demo
-- seed): a trigger catches all of them and any future one.
-- The payload is the id only (NOTIFY caps a payload at 8000 bytes); the listener reloads the
-- row by id. NOTIFY is sent on commit, so a rolled back insert announces nothing.
CREATE OR REPLACE FUNCTION notifications_announce() RETURNS trigger AS $$
BEGIN
    PERFORM pg_notify('otw_notifications', NEW.id::text);
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER notifications_announce
    AFTER INSERT ON notifications
    FOR EACH ROW EXECUTE FUNCTION notifications_announce();
