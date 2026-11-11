# Error report server

Easy Sound Cues sends error reports here (see `ENDPOINT` in `src/reporter.rs`). It's a
[Cloudflare Worker](https://developers.cloudflare.com/workers/) with a D1 database:

- `POST /report` takes a report (JSON, up to 64 KB), keeps it for 90 days, and rate-limits each
  sender (by a salted hash of their IP address; the address itself is never stored).
- `GET /?key=…` lists reports, grouped by problem. Without the key there's nothing to see.
- If email is set up (an `EMAIL` send_email binding plus `NOTIFY_EMAIL`), each new problem is
  also emailed, at most once an hour per problem.

To run your own: `wrangler d1 create easy-sound-cues-reports`, put its id in `wrangler.jsonc`,
`wrangler d1 execute easy-sound-cues-reports --remote --file schema.sql`, set the secrets listed
in `wrangler.jsonc` with `wrangler secret put`, then `wrangler deploy`, and point `ENDPOINT` at it.
