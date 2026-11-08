# Parzi mobile

Phone and desktop work together through your Parzi server, the way T3 Code
does it: the server holds the sessions, and every device is a client of it.
Android first, built with Tauri 2.

## Using the gateway today

On the server:

```bash
parzi gateway on            # listens on 127.0.0.1:7337; --lan opens it to the network
parzi devices add "Pixel"   # prints the device's token, once
parzi devices list
parzi devices revoke Pixel
```

A client opens a WebSocket to the gateway and sends
`{"op":"hello","token":"pzd_…"}` first. Then it sends requests
`{"op":"session.list","rid":1}` and `{"op":"subscribe"}` for events, as the
SSH relay does. Reading needs the `read` level, acting needs `operate`, and
the rest (approving every tool at once, deleting, settings, stopping the
engine) needs `admin`.

## How it fits together

- **The server** runs `parzi serve` (ParziOS) and is the one place sessions
  live. Today it is reached only over SSH and listens on localhost.
- **The gateway** is a WebSocket listener in `parzi serve`, off by default. It
  speaks the same methods and events as the SSH relay (`{op, rid, ...}` in,
  `{rid, ok, ...}` and `{event}` out), so the desktop's remote client and the
  phone share one protocol.
- **Devices** pair once with a one-time code or QR and get their own token,
  kept only as a hash on the server, with a level (read, operate, admin) and
  a way to revoke it.
- **Reach** is a private network first: Tailscale Serve gives HTTPS inside your
  tailnet with no ports opened, and the local network works too. A hosted
  relay can come later.
- **Push** for approvals, questions and finished runs goes through
  UnifiedPush (the free ntfy app), so there is no push server to run.

## Now: the server becomes a hub

1. **Gateway in the engine.** *(done: `parzi gateway on`, `parzi devices add`)* Opt-in WebSocket listener, the same methods and
   events, device tokens. Done when a test connects, lists sessions, streams
   a run, and a bad token is refused.
2. **Pairing and devices.** `parzi pair` prints a QR; the one-time code becomes
   a per-device token. Devices in Settings and `parzi devices revoke`. Done
   when a code works once and expires, and a revoked device is cut off.
3. **No lost events.** Each session's events are numbered; a client resumes
   from the last one it saw. Done when dropping the connection mid-run and
   reconnecting leaves no gap.
4. **Easy reach.** `parzi serve --tailscale` and the local network; clients
   try their routes in order. Done when a phone connects over a tailnet with
   no ports opened.

## Next: the Android app

5. **Tauri 2 Android shell.** A Rust WebSocket client (the desktop's remote
   client, over the gateway), the token in Android's keystore. Done when it
   pairs by QR and lists sessions.
6. **Phone layout of the UI.** Sessions, the thread with charts and plan
   cards, the composer with model and effort.
7. **Approvals, questions and push.** UnifiedPush notifications open the card;
   answering on the phone unblocks the run on the server.
8. **Desktop as a peer.** The desktop can connect through the gateway too and
   shows "Pair a phone" with a QR. A session started on either shows live on
   both.

## Later

9. **Parzi Connect.** A hosted relay so no VPN is needed.
10. **iPhone.**
11. **Play Store**, with the APK built by the release workflow.
