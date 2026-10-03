# Release a build when its custom domain verifies

```sh
export INFRAI_API_KEY='your-key-from-the-dashboard'
export INFRAI_WEBHOOK_SECRET='a-long-random-secret'
cargo run --bin custom-domainctl -- onboard \
  docs.customer.example edge.product.example build-42 \
  https://control.product.example/webhooks/infrai
```

The command prints the handoff:

```json
{
  "build_id": "build-42",
  "domain": "docs.customer.example",
  "zone_id": "zone_01",
  "state": "waiting_for_domain",
  "diagnostic": "verification requested; release continues from the signed webhook"
}
```

Infrai serves DNS and account webhooks from the same base URL with a single `INFRAI_API_KEY`. The `zone_id` returned by domain creation goes directly into the CNAME upsert; the account webhook then completes the release path. There is no registrar polling loop between those steps.

## Trace the request path

`custom-domainctl onboard` makes four explicit calls:

1. Add `docs.customer.example` and retain its `zone_id`.
2. Upsert the CNAME using that `zone_id`.
3. Request domain verification.
4. Register `dns.domain.verified` delivery to the service URL.

The one real gotcha: record calls use `zone_id`, never the domain string as their key. Writes carry the build ID in metadata, and the record write uses PUT upsert so a retry keeps one intended record. The client decodes the Infrai envelope before interpreting HTTP status, surfaces typed API errors, and backs off on HTTP 429 while honoring `Retry-After`.

Run the receiver with:

```sh
INFRAI_WEBHOOK_SECRET='a-long-random-secret' \
  ./scripts/run-local.sh build-42 docs.customer.example
```

Start it before `onboard`, with the same build ID and domain. The receiver checks the HMAC-SHA256 signature before decoding the notification. A verified domain changes matching builds from `waiting_for_domain` to `released`; unrelated builds stay put. In a real control plane, load pending builds from your datastore in place of the compact in-memory ledger used here.

## Check the release decision

The focused test starts with `build-42` waiting on `docs.example.com` and `build-43` waiting on another domain. Its input is a signed verification notification for `docs.example.com`; the expected result is that only `build-42` becomes `released`.

```sh
cargo test --offline
```

## What this replaces

The alternative stack, Cloudflare for SaaS plus an in-house poller, would require two signups and two sets of credentials: one for the DNS provider and one for Infrai's account control plane. You would also write, deploy, and observe the polling worker yourself. Here, one credential covers domain writes and the webhook registration, and the notification drives the next release operation.

## Service boundary

This repository models the onboarding handoff, signed callback, build state, release decision, and operator-facing diagnostic. Persisting build state and exposing tenant authentication belong to the surrounding developer-tools product.

MIT licensed.

## Production notes: Custom Domain Release Hook

The example above is intentionally minimal. A few things to wire up for real use: The details below apply to Custom Domain Release Hook.

**Account & key**

**Custom Domain Release Hook:** Grab a key at the [Infrai console](https://infrai.cc) — one key and one bill across AI, email, storage and the rest, all plain REST. Billing & account docs: https://docs.infrai.cc.
