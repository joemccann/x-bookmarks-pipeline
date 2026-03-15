#!/usr/bin/env node
/**
 * X Bookmarks Pipeline — Email Notifier
 *
 * Usage:
 *   node bin/notify.mjs --mode error --message "..." --cycle 123
 *   echo '{"bookmarks":[...],"cycle":123}' | node bin/notify.mjs --mode bookmarks
 *
 * Required env vars (shared with transparent-classroom-image-downloader):
 *   SMTP_HOST   SMTP_PORT   SMTP_USER   SMTP_PASS
 *   EMAIL_FROM  EMAIL_TO
 */

import { createInterface } from "node:readline";
import nodemailer from "nodemailer";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function escapeHtml(value) {
  return String(value)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

function parseArgs(argv) {
  const opts = {};
  for (let i = 0; i < argv.length; i++) {
    if (argv[i].startsWith("--")) {
      opts[argv[i].slice(2)] = argv[i + 1] ?? "";
      i++;
    }
  }
  return opts;
}

function createTransporter() {
  const host = process.env.SMTP_HOST;
  const port = parseInt(process.env.SMTP_PORT || "587", 10);
  const user = process.env.SMTP_USER;
  const pass = process.env.SMTP_PASS;

  if (!host || !user || !pass) {
    throw new Error("Missing required env vars: SMTP_HOST, SMTP_USER, SMTP_PASS");
  }

  return nodemailer.createTransport({
    host,
    port,
    secure: port === 465,
    auth: { user, pass },
  });
}

function resolveAddresses() {
  const from = process.env.EMAIL_FROM;
  const to = process.env.EMAIL_TO;
  if (!from) throw new Error("Missing EMAIL_FROM env var");
  if (!to) throw new Error("Missing EMAIL_TO env var");
  return { from, to };
}

async function readStdin() {
  const rl = createInterface({ input: process.stdin, crlfDelay: Infinity });
  const lines = [];
  for await (const line of rl) lines.push(line);
  return lines.join("\n");
}

// ---------------------------------------------------------------------------
// Error email
// ---------------------------------------------------------------------------

function buildErrorHtml({ message, cycle, timestamp }) {
  return `<!doctype html>
<html>
<body style="margin:0;padding:24px;background:#f8fafc;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;color:#0f172a;">
  <div style="max-width:600px;margin:0 auto;background:#fff;border:1px solid #dbe4ee;border-radius:18px;overflow:hidden;">
    <div style="padding:24px 28px;background:linear-gradient(135deg,#7f1d1d 0%,#991b1b 100%);color:#fff;">
      <div style="font-size:11px;letter-spacing:0.08em;text-transform:uppercase;opacity:0.78;">X Bookmarks Pipeline</div>
      <h1 style="margin:8px 0 0;font-size:22px;line-height:1.3;">Token Refresh Failed</h1>
    </div>
    <div style="padding:28px;">
      <div style="padding:16px 20px;background:#fff5f5;border:1px solid #feb2b2;border-radius:12px;margin-bottom:20px;">
        <div style="font-size:11px;letter-spacing:0.08em;text-transform:uppercase;color:#9a3412;font-weight:700;margin-bottom:8px;">Error</div>
        <pre style="margin:0;color:#742a2a;font-size:13px;white-space:pre-wrap;word-break:break-word;font-family:ui-monospace,monospace;">${escapeHtml(message)}</pre>
      </div>
      <div style="padding:16px 20px;background:#fffbeb;border:1px solid #fcd34d;border-radius:12px;margin-bottom:20px;">
        <div style="font-size:11px;letter-spacing:0.08em;text-transform:uppercase;color:#92400e;font-weight:700;margin-bottom:8px;">Action Required</div>
        <p style="margin:0;color:#0f172a;line-height:1.6;">
          The X OAuth refresh token has expired or been revoked. Run
          <code style="background:#f1f5f9;padding:2px 6px;border-radius:4px;font-size:13px;font-family:ui-monospace,monospace;">python bin/auth_pkce.py</code>
          in the project directory to re-authenticate and update <code style="background:#f1f5f9;padding:2px 6px;border-radius:4px;font-size:13px;font-family:ui-monospace,monospace;">.env</code>.
        </p>
      </div>
      <table style="font-size:13px;color:#475569;border-collapse:collapse;">
        <tr><td style="padding:3px 16px 3px 0;white-space:nowrap;">Poll cycle</td><td style="color:#0f172a;">${escapeHtml(String(cycle ?? "(unknown)"))}</td></tr>
        <tr><td style="padding:3px 16px 3px 0;">Occurred at</td><td style="color:#0f172a;">${escapeHtml(timestamp)}</td></tr>
      </table>
    </div>
  </div>
</body>
</html>`;
}

async function sendErrorEmail({ message, cycle }) {
  const transporter = createTransporter();
  const { from, to } = resolveAddresses();
  const timestamp = new Date().toLocaleString();

  await transporter.sendMail({
    from,
    to,
    subject: "⚠️ X Bookmarks Pipeline: Token Refresh Failed",
    text: [
      "X Bookmarks Pipeline — Token Refresh Failed",
      "",
      `Error: ${message}`,
      "",
      "Action required: run `python bin/auth_pkce.py` to re-authenticate.",
      "",
      `Poll cycle:  ${cycle ?? "(unknown)"}`,
      `Occurred at: ${timestamp}`,
    ].join("\n"),
    html: buildErrorHtml({ message, cycle, timestamp }),
  });
}

// ---------------------------------------------------------------------------
// Bookmark digest email
// ---------------------------------------------------------------------------

function buildBookmarkCard(bm, index) {
  const cat = [bm.category, bm.subcategory].filter(Boolean).join(" / ");
  const authorLink = bm.tweet_url
    ? `<a href="${escapeHtml(bm.tweet_url)}" style="color:#2563eb;text-decoration:none;font-weight:600;">@${escapeHtml(bm.author)}</a>`
    : `<span style="font-weight:600;">@${escapeHtml(bm.author)}</span>`;

  let validBadge = "";
  if (bm.valid === true) {
    validBadge = `<span style="display:inline-block;padding:1px 8px;background:#dcfce7;color:#166534;border-radius:6px;font-size:11px;font-weight:700;margin-left:6px;">VALID</span>`;
  } else if (bm.valid === false) {
    validBadge = `<span style="display:inline-block;padding:1px 8px;background:#fee2e2;color:#991b1b;border-radius:6px;font-size:11px;font-weight:700;margin-left:6px;">INVALID</span>`;
  }

  const planLine = bm.plan_title
    ? `<div style="font-size:12px;color:#6b7280;margin-top:3px;">Plan: ${escapeHtml(bm.plan_title)}</div>`
    : "";

  const excerpt = bm.text_excerpt
    ? `<div style="margin-top:10px;color:#374151;font-size:14px;line-height:1.55;border-left:3px solid #e5e7eb;padding-left:10px;">${escapeHtml(bm.text_excerpt)}</div>`
    : "";

  const bg = bm.is_finance ? "#f0f9ff" : "#f9fafb";
  const border = bm.is_finance ? "#bae6fd" : "#e5e7eb";

  return `<div style="padding:16px;background:${bg};border:1px solid ${border};border-radius:12px;margin-bottom:12px;">
    <div style="display:flex;align-items:baseline;gap:8px;flex-wrap:wrap;">
      <span style="font-size:12px;color:#94a3b8;min-width:20px;">${index + 1}.</span>
      ${authorLink}${validBadge}
      <span style="font-size:12px;color:#64748b;">${escapeHtml(cat)}</span>
    </div>
    ${planLine}
    ${excerpt}
  </div>`;
}

function buildDigestHtml({ bookmarks, cycle, timestamp }) {
  const total = bookmarks.length;
  const finance = bookmarks.filter((b) => b.is_finance).length;
  const valid = bookmarks.filter((b) => b.valid === true).length;
  const cards = bookmarks.map((bm, i) => buildBookmarkCard(bm, i)).join("\n");

  return `<!doctype html>
<html>
<body style="margin:0;padding:24px;background:#f8fafc;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;color:#0f172a;">
  <div style="max-width:680px;margin:0 auto;background:#fff;border:1px solid #dbe4ee;border-radius:18px;overflow:hidden;">
    <div style="padding:24px 28px;background:linear-gradient(135deg,#1e3a5f 0%,#1e40af 100%);color:#fff;">
      <div style="font-size:11px;letter-spacing:0.08em;text-transform:uppercase;opacity:0.78;">X Bookmarks Pipeline</div>
      <h1 style="margin:8px 0 4px;font-size:22px;line-height:1.3;">${total} New Bookmark${total === 1 ? "" : "s"} Processed</h1>
      <p style="margin:0;opacity:0.85;font-size:13px;">
        ${finance} finance${finance !== 1 ? "s" : ""} · ${valid} valid Pine Script${valid !== 1 ? "s" : ""} · Cycle ${cycle ?? "?"} · ${escapeHtml(timestamp)}
      </p>
    </div>
    <div style="padding:28px;">
      ${cards}
    </div>
  </div>
</body>
</html>`;
}

async function sendBookmarksDigest({ bookmarks, cycle }) {
  const transporter = createTransporter();
  const { from, to } = resolveAddresses();
  const timestamp = new Date().toLocaleString();
  const total = bookmarks.length;
  const finance = bookmarks.filter((b) => b.is_finance).length;

  const textLines = [
    `X Bookmarks Pipeline — ${total} New Bookmark${total === 1 ? "" : "s"} Processed`,
    `Cycle ${cycle ?? "?"} · ${timestamp}`,
    "",
    ...bookmarks.map((bm, i) => {
      const cat = [bm.category, bm.subcategory].filter(Boolean).join(" / ");
      const valid =
        bm.valid === true ? " [VALID]" : bm.valid === false ? " [INVALID]" : "";
      const plan = bm.plan_title ? `\n   Plan: ${bm.plan_title}` : "";
      const excerpt = bm.text_excerpt ? `\n   ${bm.text_excerpt}` : "";
      return `${i + 1}. @${bm.author} — ${cat}${valid}${plan}${excerpt}`;
    }),
  ];

  await transporter.sendMail({
    from,
    to,
    subject: `📌 X Bookmarks: ${total} processed (${finance} finance) — Cycle ${cycle ?? "?"}`,
    text: textLines.join("\n"),
    html: buildDigestHtml({ bookmarks, cycle, timestamp }),
  });
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const mode = args.mode;

  if (!mode) {
    console.error(
      "Usage:\n" +
        "  node bin/notify.mjs --mode error --message TEXT --cycle N\n" +
        "  echo JSON | node bin/notify.mjs --mode bookmarks"
    );
    process.exitCode = 1;
    return;
  }

  try {
    if (mode === "error") {
      const message = args.message || "(no message)";
      const cycle = args.cycle ? parseInt(args.cycle, 10) : undefined;
      await sendErrorEmail({ message, cycle });
      console.log("Error alert sent.");
    } else if (mode === "bookmarks") {
      const raw = await readStdin();
      const { bookmarks, cycle } = JSON.parse(raw);
      await sendBookmarksDigest({ bookmarks, cycle });
      console.log(`Bookmark digest sent (${bookmarks.length} items).`);
    } else {
      throw new Error(`Unknown --mode: ${mode}`);
    }
  } catch (err) {
    console.error(`notify.mjs error: ${err.message}`);
    process.exitCode = 1;
  }
}

main();
