# platform

The customer dashboard. Nothing is here yet; this README describes what will be. Planned stack: TanStack Start, deployed to Cloudflare Workers.

## Purpose

Give developers one place to set up and operate push: connect their apps, upload Apple and Google credentials, issue API keys, send a test, and see exactly what happened to every notification and device.

## Scope

In scope:
- Sign-in, organisations and team members (how is open decision A3 in `apps/engine/ARCHITECTURE.md`).
- Projects, environments and apps; uploading, verifying and rotating APNs keys and FCM service accounts.
- API keys: create, reveal once, revoke.
- Test sends, which onboarding requires before going live.
- Delivery history: notifications, per-device deliveries, every attempt with the provider's raw reason.
- Templates, usage and billing.

Out of scope: any delivery logic or direct database access. Everything goes through the engine's management API, using a typed client generated from the engine's OpenAPI spec.
