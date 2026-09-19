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
  clicks About, reports DOM state and console errors. COOKIE is the bare
  cookie *value*, not `atrium=…`. Start Chromium with
  `--headless=new --no-sandbox --remote-debugging-port=9222` first
  (without `--no-sandbox` it aborts on this host).
- `fakebin/` — put it first on PATH before `fake_atriumd.py` and apps that
  exec `svctl` get the fake in `fakebin/svctl`: canned services, state in
  `$FAKE_SVCTL_STATE`, failure modes via a `mode` key in that file.
  `scen_services.py` drives the Services app against it.
- `fakebin/peipkg` is not a fake: it execs a real peipkg
  (`$FAKE_PEIPKG_BIN`) on a scratch root (`$FAKE_PEIPKG_ROOT`), so
  `scen_packages.py` installs, refuses and undoes for real. The file says
  how to give the root a repository. A package carrying security-descriptor
  overrides cannot be installed on a plain host (no `security.peios.sd`);
  the scenario uses one that can.
- Building `atrium-session` off Peios needs libpeios found by hand: a
  `peios.pc` filled in from `libpeios/peios.pc.in` (prefix the libpeios
  tree, libdir its `target/release`) on `PKG_CONFIG_PATH`, and
  `BINDGEN_EXTRA_CLANG_ARGS="-I$(gcc -print-file-name=include) -I…/pkm/uapi"`.
- `ffdrive.py COOKIE URL` — the same through geckodriver on :4444.
- `CDP_SCENARIO=scen_windows.py cdp.py …` runs a scenario file instead of
  the default click-About check (`ev`, `time`, `logs` are in scope).

The last two work against the fake stack and against a running VM on
`localhost:8080` alike; log in with curl to get the cookie
(`POST /api/logon/start`, then `/api/logon/answer`).
