-- Error reports sent by Easy Sound Cues. Rows older than 90 days are deleted by the Worker.
CREATE TABLE IF NOT EXISTS reports (
  id TEXT PRIMARY KEY,
  received INTEGER NOT NULL,   -- seconds since 1970
  ip_hash TEXT NOT NULL,       -- salted hash of the sender's IP, only for rate limiting
  install TEXT,                -- random id the app made up on first launch
  version TEXT,
  os TEXT,
  stage TEXT,
  kind TEXT,                   -- error, panic, test or user
  message TEXT,
  signature TEXT,              -- groups reports of the same problem
  body TEXT NOT NULL           -- the whole report as the app sent it
);
CREATE INDEX IF NOT EXISTS reports_received ON reports (received);
CREATE INDEX IF NOT EXISTS reports_ip ON reports (ip_hash, received);
CREATE INDEX IF NOT EXISTS reports_signature ON reports (signature, received);

-- When each problem was last announced, so the same one doesn't email every few seconds.
CREATE TABLE IF NOT EXISTS notified (
  signature TEXT PRIMARY KEY,
  at INTEGER NOT NULL
);
