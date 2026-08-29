# Development harness

Everything here runs on a plain Linux host with no KACS, no authd and no
peinit, so the parts of Atrium above the kernel boundary can be exercised
with curl and a browser.

- `fake_atriumd.py SERVER SESSION` — stands in for atriumd: spawns the real
  `atrium-server` on fd 3, answers the logon conversation (any user,
  password `secret`), and on grant spawns the real `atrium-session` with a
  socketpair, passing its end to the server like atriumd does. Honours
  `ATRIUM_LISTEN`, `ATRIUM_APPS_DIR`, `LD_LIBRARY_PATH` (needs a
  `libpeios.so.0` symlink to `libpeios/target/release/libpeios.so`).
- `fake_peinit.py SOCKET` — a jobs manager (PSPU §7 subset) for
  `cargo test -p atriumd` with `ATRIUM_JOBS_SOCKET=SOCKET`.
- `wsclient.py COOKIE` — raw-socket websocket client for the session mirror.
- `cdp.py COOKIE URL` — drives the Playwright Chromium in
  `~/.cache/ms-playwright` over CDP: sets the cookie, loads the shell,
  clicks About, reports DOM state and console errors. Start Chromium with
  `--headless=new --remote-debugging-port=9222` first.
- `ffdrive.py COOKIE URL` — the same through geckodriver on :4444.
- `CDP_SCENARIO=scen_windows.py cdp.py …` runs a scenario file instead of
  the default click-About check (`ev`, `time`, `logs` are in scope).

The last two work against the fake stack and against a running VM on
`localhost:8080` alike; log in with curl to get the cookie
(`POST /api/logon/start`, then `/api/logon/answer`).
