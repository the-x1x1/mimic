-- The three themes are named for what they look like rather than for a
-- brightness: plain, paper, night. An install from 0.8.0 or earlier has
-- 'dark' or 'light' stored under general.theme, and the contract that parses
-- settings no longer accepts either, so the value is carried over here rather
-- than being left to fail validation on the first render after an upgrade.
--
-- 'dark' becomes 'night', which is the same intent. 'light' becomes 'plain',
-- the new default, rather than 'paper', because paper is a deliberate choice
-- and nobody who stored 'light' made it.
UPDATE app_settings SET value_json = '"night"'
 WHERE key = 'general.theme' AND value_json = '"dark"';

UPDATE app_settings SET value_json = '"plain"'
 WHERE key = 'general.theme' AND value_json = '"light"';

-- Anything else stored under that key is not a theme this build can render.
UPDATE app_settings SET value_json = '"plain"'
 WHERE key = 'general.theme'
   AND value_json NOT IN ('"plain"', '"paper"', '"night"');
