# Security policy

## Supported versions

Redrover is pre-1.0. Only the latest commit on `main` receives security
fixes.

## Reporting a vulnerability

For non-sensitive bugs, please [open an issue](https://github.com/hdrover/discord-drover/issues/new/choose).

For anything you'd rather not disclose publicly — credential leakage,
remote code execution, anything that could be used to attack other users
running Redrover — please email the maintainers directly rather than
filing a public issue. We will:

1. Acknowledge receipt within a reasonable timeframe.
2. Coordinate a fix and a disclosure window.
3. Credit you in the changelog if you'd like.

## Threat model

Redrover is a **user-space** tool that runs in the same security context
as the Discord client. It:

- Does not require admin rights and does not modify the OS.
- Does not perform privilege escalation.
- Loads a DLL into Discord's process; that DLL has the same access as
  any other code Discord runs.
- Talks to a proxy server the user explicitly configures. Credentials
  are stored in `drover.ini` in **plain text** — by design, since the
  Pascal predecessor did the same and there's no clear win to
  obfuscating them when the same user can read the file anyway.

What we do **not** defend against:

- A malicious local user with read access to `drover.ini` (they get
  the proxy credentials — same as reading a browser's saved password
  file).
- A malicious proxy server (it can MITM everything Discord sends
  through TLS-terminated paths, though for HTTPS Chromium does
  certificate validation against the real Discord cert).
- Anti-cheat or anti-tamper systems that flag DLL injection. Redrover
  is unsigned and uses standard `LoadLibrary` paths; it will be
  detected.

## What's in scope

- The Windows `version.dll`, the POSIX preload library.
- The Rust GUI installer.
- The `drover.ini` parser (both sides).

## What's out of scope

- Discord's own security model.
- The user's chosen proxy server.
- The proxy protocol's own weaknesses (SOCKS5 has no built-in encryption).
