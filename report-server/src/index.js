// Collects Easy Sound Cues error reports, keeps them for 90 days, shows them on a private page,
// and (once email is set up) emails the app's maker about new problems.
//
// Reports are anonymous: the app removes names and home folders before sending, and this server
// keeps only a salted hash of the sender's IP address (to stop floods), never the address itself.
// Reading the reports needs VIEW_KEY, which lives only in this Worker's secrets.

const MAX_BODY = 64 * 1024;
const KEEP_DAYS = 90;
const PER_IP_PER_HOUR = 30;
const PER_DAY = 2000;
// The same problem is announced at most once an hour, however many computers report it.
const ANNOUNCE_EVERY = 60 * 60;

export default {
  async fetch(request, env, ctx) {
    const url = new URL(request.url);
    if (url.pathname === "/report" && request.method === "POST") {
      return receive(request, env, ctx, url);
    }
    if (request.method === "GET" && (url.pathname === "/" || url.pathname.startsWith("/r/"))) {
      if (!(await authorised(url, env))) {
        return text("Easy Sound Cues error reports are received here. They aren't public.", 200);
      }
      return url.pathname === "/" ? list(url, env) : detail(decodeURIComponent(url.pathname.slice(3)), url, env);
    }
    return text("Not found", 404);
  },
};

function text(body, status) {
  return new Response(body, { status, headers: { "content-type": "text/plain; charset=utf-8" } });
}

function json(body, status) {
  return new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } });
}

async function sha256(value) {
  const data = new TextEncoder().encode(value);
  const hash = await crypto.subtle.digest("SHA-256", data);
  return [...new Uint8Array(hash)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

/** Compare the ?key= in the link with VIEW_KEY without leaking how much of it matched. */
async function authorised(url, env) {
  const given = url.searchParams.get("key") || "";
  if (!env.VIEW_KEY || !given) return false;
  const [a, b] = await Promise.all([sha256(given), sha256(env.VIEW_KEY)]);
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return diff === 0;
}

function clip(value, length) {
  return typeof value === "string" ? value.slice(0, length) : "";
}

async function receive(request, env, ctx, url) {
  const raw = await request.text();
  if (raw.length > MAX_BODY) return json({ ok: false, error: "too big" }, 413);
  let report;
  try {
    report = JSON.parse(raw);
  } catch {
    return json({ ok: false, error: "not JSON" }, 400);
  }
  if (!report || report.app !== "easy-sound-cues" || typeof report.message !== "string") {
    return json({ ok: false, error: "not a report" }, 400);
  }
  const now = Math.floor(Date.now() / 1000);
  const ip = request.headers.get("cf-connecting-ip") || "unknown";
  const ipHash = (await sha256(`${env.IP_SALT || "easy-sound-cues"}:${ip}`)).slice(0, 24);

  const recent = await env.DB.prepare(
    "SELECT (SELECT COUNT(*) FROM reports WHERE ip_hash = ?1 AND received > ?2) AS mine, " +
      "(SELECT COUNT(*) FROM reports WHERE received > ?3) AS everyone",
  )
    .bind(ipHash, now - 3600, now - 86400)
    .first();
  if (recent.mine >= PER_IP_PER_HOUR || recent.everyone >= PER_DAY) {
    return json({ ok: false, error: "too many reports, try later" }, 429);
  }

  const stage = clip(report.stage, 120) || "unknown";
  const message = clip(report.message, 2000);
  const signature = (await sha256(`${stage}\n${message.slice(0, 200)}`)).slice(0, 16);
  const id = `${now.toString(36)}-${crypto.randomUUID().slice(0, 8)}`;
  await env.DB.batch([
    env.DB.prepare(
      "INSERT INTO reports (id, received, ip_hash, install, version, os, stage, kind, message, signature, body) " +
        "VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
    ).bind(
      id,
      now,
      ipHash,
      clip(report.install, 40),
      clip(report.version, 20),
      clip(report.os, 120),
      stage,
      clip(report.kind, 20) || "error",
      message,
      signature,
      raw,
    ),
    env.DB.prepare("DELETE FROM reports WHERE received < ?1").bind(now - KEEP_DAYS * 86400),
  ]);
  const announcement = announce(env, url, { id, now, stage, message, signature, report }).catch((e) => `failed: ${e}`);
  // A test report waits for its announcement, so "Send a test report" can say whether it arrived.
  if (report.kind === "test") return json({ ok: true, id, announced: await announcement }, 200);
  ctx.waitUntil(announcement.then((status) => console.log(`report ${id}: ${status}`)));
  return json({ ok: true, id }, 200);
}

/** Email the maker a summary, if email is set up (an EMAIL send_email binding and NOTIFY_EMAIL). */
async function announce(env, url, { id, now, stage, message, signature, report }) {
  if (!env.EMAIL || !env.NOTIFY_EMAIL) return "not announced: email isn't set up yet";
  const last = await env.DB.prepare("SELECT at FROM notified WHERE signature = ?1").bind(signature).first();
  if (last && now - last.at < ANNOUNCE_EVERY && report.kind !== "test" && report.kind !== "user") {
    return "not announced: the same problem was announced in the last hour";
  }
  await env.DB.prepare("INSERT OR REPLACE INTO notified (signature, at) VALUES (?1, ?2)").bind(signature, now).run();
  const link = `${url.origin}/r/${encodeURIComponent(id)}?key=${encodeURIComponent(env.VIEW_KEY || "")}`;
  // Mail headers have to be plain ASCII.
  const ascii = (s) => s.replace(/[^\x20-\x7e]/g, "?");
  const from = env.FROM_EMAIL || "reports@example.com";
  const subject = ascii(`Easy Sound Cues ${clip(report.version, 20)}: ${stage}`).slice(0, 180);
  const body = [
    message.slice(0, 3000),
    "",
    `System: ${clip(report.os, 120)}`,
    `Computer: ${clip(report.install, 8)} (${clip(report.kind, 20)})`,
    "",
    `Full report: ${link}`,
    `All reports: ${url.origin}/?key=${encodeURIComponent(env.VIEW_KEY || "")}`,
  ].join("\r\n");
  const raw = [
    `From: Easy Sound Cues <${from}>`,
    `To: ${env.NOTIFY_EMAIL}`,
    `Subject: ${subject}`,
    `Message-ID: <${id}@${from.split("@")[1]}>`,
    `Date: ${new Date().toUTCString()}`,
    "MIME-Version: 1.0",
    "Content-Type: text/plain; charset=utf-8",
    "Content-Transfer-Encoding: 8bit",
    "",
    body,
  ].join("\r\n");
  const { EmailMessage } = await import("cloudflare:email");
  await env.EMAIL.send(new EmailMessage(from, env.NOTIFY_EMAIL, raw));
  return "announced by email";
}

function escape(value) {
  return String(value ?? "").replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
}

function page(title, body) {
  return new Response(
    `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width">
<meta name="robots" content="noindex"><title>${escape(title)}</title><style>
body{font:15px/1.45 system-ui,sans-serif;margin:24px;background:#0c141a;color:#e8f0f3}
a{color:#2dd4bf}h1{font-size:22px}h2{font-size:17px;margin-top:28px}
table{border-collapse:collapse;width:100%}td,th{padding:6px 8px;border-bottom:1px solid #2a404b;text-align:left;vertical-align:top}
th{color:#a0b6bf;font-weight:600}.muted{color:#a0b6bf}.panic{color:#f87171}.test{color:#4ade80}
pre{background:#111c23;padding:12px;border-radius:8px;white-space:pre-wrap;word-break:break-word}
</style></head><body>${body}</body></html>`,
    {
      headers: {
        "content-type": "text/html; charset=utf-8",
        "content-security-policy": "default-src 'none'; style-src 'unsafe-inline'",
        "cache-control": "no-store",
      },
    },
  );
}

function when(seconds) {
  return new Date(seconds * 1000).toISOString().replace("T", " ").slice(0, 16) + " UTC";
}

async function list(url, env) {
  const key = encodeURIComponent(url.searchParams.get("key"));
  const weekAgo = Math.floor(Date.now() / 1000) - 7 * 86400;
  const [common, latest] = await Promise.all([
    env.DB.prepare(
      "SELECT signature, COUNT(*) AS n, COUNT(DISTINCT install) AS computers, MAX(received) AS last, " +
        "MAX(stage) AS stage, MAX(message) AS message, MAX(id) AS id FROM reports WHERE received > ?1 " +
        "GROUP BY signature ORDER BY n DESC LIMIT 20",
    )
      .bind(weekAgo)
      .all(),
    env.DB.prepare(
      "SELECT id, received, install, version, os, stage, kind, message FROM reports ORDER BY received DESC LIMIT 200",
    ).all(),
  ]);
  const commonRows = common.results
    .map(
      (r) => `<tr><td>${r.n}</td><td>${r.computers}</td><td>${escape(r.stage)}</td>
<td><a href="/r/${encodeURIComponent(r.id)}?key=${key}">${escape(r.message.slice(0, 160))}</a></td><td class="muted">${when(r.last)}</td></tr>`,
    )
    .join("");
  const latestRows = latest.results
    .map(
      (r) => `<tr><td class="muted">${when(r.received)}</td><td>${escape(r.version)}</td><td>${escape(r.os)}</td>
<td class="${escape(r.kind)}">${escape(r.stage)}</td><td><a href="/r/${encodeURIComponent(r.id)}?key=${key}">${escape(r.message.slice(0, 160))}</a></td>
<td class="muted">${escape((r.install || "").slice(0, 8))}</td></tr>`,
    )
    .join("");
  return page(
    "Easy Sound Cues reports",
    `<h1>Easy Sound Cues error reports</h1>
<p class="muted">Kept for ${KEEP_DAYS} days. Each “computer” is a random id an installation made up for itself.</p>
<h2>Most common problems this week</h2>
<table><tr><th>Reports</th><th>Computers</th><th>Where</th><th>Problem</th><th>Last seen</th></tr>${commonRows || '<tr><td colspan="5" class="muted">None. 🎉</td></tr>'}</table>
<h2>Latest reports</h2>
<table><tr><th>When</th><th>Version</th><th>System</th><th>Where</th><th>Problem</th><th>Computer</th></tr>${latestRows || '<tr><td colspan="6" class="muted">None yet.</td></tr>'}</table>`,
  );
}

async function detail(id, url, env) {
  const row = await env.DB.prepare("SELECT * FROM reports WHERE id = ?1").bind(id).first();
  if (!row) return text("No such report", 404);
  let report = {};
  try {
    report = JSON.parse(row.body);
  } catch {}
  const back = `/?key=${encodeURIComponent(url.searchParams.get("key"))}`;
  const lines = Array.isArray(report.log) ? report.log.map(escape).join("\n") : "";
  const facts = [
    ["When", when(row.received)],
    ["Version", row.version],
    ["System", row.os],
    ["Where", row.stage],
    ["Kind", row.kind],
    ["Computer", row.install],
  ]
    .map(([k, v]) => `<tr><th>${k}</th><td>${escape(v)}</td></tr>`)
    .join("");
  return page(
    `Report ${id}`,
    `<p><a href="${back}">← All reports</a></p><h1>${escape(row.stage)}</h1><table>${facts}</table>
<h2>What went wrong</h2><pre>${escape(row.message)}</pre>
${report.detail ? `<h2>Details</h2><pre>${escape(report.detail)}</pre>` : ""}
${lines ? `<h2>What the app did just before</h2><pre>${lines}</pre>` : ""}
<h2>Everything that was sent</h2><pre>${escape(JSON.stringify(report, null, 2))}</pre>`,
  );
}
