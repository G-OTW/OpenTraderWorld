-- A robot claims its instruments on its account when it is armed, not when it is created:
-- any number of robots may name one instrument, one at a time sends. Release the claims of
-- the robots that are not armed.
DELETE FROM robot_exec_instruments i USING robots r WHERE r.id = i.session_id AND r.armed_at IS NULL;
