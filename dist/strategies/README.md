# UDP strategy payloads

This directory ships curated `drover-packet.bin`-style payloads that pair
with the `strategy = ...` selector in `drover.ini`.

| File          | `strategy = ` value | Purpose |
| ------------- | ------------------- | ------- |
| `uae-v1.bin`  | `uae_v1`            | Placeholder for UAE Etisalat / du payload (ISSUE-65-IDEAS.md §2.4). |
| `uae-v2.bin`  | `uae_v2`            | Placeholder for UAE variant 2. |

Both files are intentionally empty in the repo — they're meant to be
replaced with real bytes by people who actually have access to networks
that need them. The DLL gracefully falls back to a no-op when the file
is empty or missing.

Drop a payload here and re-run the installer; the file will be copied
into every Discord `app-*` folder alongside `version.dll`.
