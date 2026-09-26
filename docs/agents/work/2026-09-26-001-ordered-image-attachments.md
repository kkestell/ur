# Ordered image attachments

## Plan

`docs/agents/plans/2026-09-26-001-ordered-image-attachments.md`

## Summary

Image attachments now reserve their drop positions while the webview reads them. Send and Enter wait
for all remaining reads, and removing a pending attachment prevents its late result from joining a
prompt. OX-0010 is fixed.

## Decisions

- The end-to-end harness holds `FileReader` starts in the browser, then releases them in a chosen
  order. This reproduces the races without relying on file sizes or machine timing.
- The existing image prompt test now waits until its chip is ready before sending. Two new tests own
  the pending-read and drop-order guarantees. No owning tests were removed or moved.

## Automated checks

- `make e2e` — 56 passed, 0 failed.
- `make check` — passed all checks.

## Manual verification

No separate manual check was needed. Reproduce the GUI behavior and regression checks with:

```sh
make e2e
```
