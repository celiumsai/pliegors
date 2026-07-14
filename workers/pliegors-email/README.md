# PliegoRS email Worker

Cloudflare Email Routing Worker for the literal public address
`hello@pliegors.dev`. It forwards accepted messages to one verified destination
and sends a restrained bilingual acknowledgement when the sender appears human.
It never parses, stores, or logs message content.

## Safety contract

- Only the exact `hello@pliegors.dev` envelope recipient is accepted.
- Forwarding completes before an acknowledgement is attempted.
- Auto-generated mail, mailing lists, same-domain senders, suppressed replies,
  and oversized reference chains are never auto-replied to.
- Reply rejection does not discard a message already forwarded to a human.
- `FORWARD_TO` is a Wrangler secret and is never committed.
- Logs contain outcome codes and byte counts, not addresses, subjects, or body.

## Local verification

```sh
npm ci
npm run check
npm run dev
```

Wrangler can simulate an incoming email at
`/cdn-cgi/handler/email?from=person@example.com&to=hello@pliegors.dev` using a
raw RFC 5322 request body with a `Message-ID` header.

## Cloudflare setup

Do not deploy from CI. A maintainer performs these steps after reviewing the
Cloudflare account and destination:

1. Enable Email Routing for `pliegors.dev` and verify the human destination
   mailbox in Cloudflare.
2. Run `npx wrangler secret put FORWARD_TO` and enter that verified mailbox.
3. Run `npm run deploy` from this directory.
4. Create one literal Email Routing rule:
   `hello@pliegors.dev` -> Worker -> `pliegors-email`.
5. Send human, DMARC-failing, automated, and wrong-recipient test messages;
   verify forwarding, loop suppression, and logs before announcing the address.

The staging Worker uses `npm run deploy:staging` and has a separate
`FORWARD_TO` secret. Email Routing rules are configured in Cloudflare and are
not represented by an HTTP route in `wrangler.jsonc`.
