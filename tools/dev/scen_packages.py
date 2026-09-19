# The Packages app against fakebin/peipkg — the real peipkg on a scratch root
# (see that file for FAKE_PEIPKG_BIN / FAKE_PEIPKG_ROOT). Expects the root to
# hold org.gnu.gcc-libgcc already, so there is something to list, inspect and
# refuse to orphan. PACKAGES_SHOTS=DIR saves screenshots there.
import base64, os

SHOTS = os.environ.get('PACKAGES_SHOTS')
# Small and free of security-descriptor overrides, which a plain host cannot
# stamp — so the install succeeds here as it would on Peios.
PKG = 'io.github.zlib-ng.zlib-ng'

call('Emulation.setDeviceMetricsOverride', width=1440, height=960, deviceScaleFactor=1, mobile=False)
for _ in range(20):
    if ev('[...document.querySelectorAll(".tbA-tile")].some(t=>t.dataset.id==="org.peios.packages")'): break
    time.sleep(0.5)
else:
    raise SystemExit('packages tile never appeared')
ev('[...document.querySelectorAll(".tbA-tile")].find(t=>t.dataset.id==="org.peios.packages").click()'); time.sleep(4)
F = 'document.querySelector("#ws-frames iframe")'
def fr(js): return ev(f'(()=>{{ const d={F}.contentDocument; return ({js}); }})()')
def click(sel, wait=1.5):
    fr(f'(d.querySelector(`{sel}`).click(), 1)'); time.sleep(wait)
def typ(iid, text, wait=1.5):
    fr(f'(d.getElementById("{iid}").value = {text!r}, d.getElementById("{iid}").dispatchEvent(new Event("input")), 1)'); time.sleep(wait)
def rows(view): return fr(f'[...d.querySelectorAll("#rows-{view} tr")].map(r => r.textContent.replace(/\\s+/g, " ").trim())')
def shot(name):
    if not SHOTS: return
    data = call('Page.captureScreenshot', format='png')['data']
    open(os.path.join(SHOTS, name + '.png'), 'wb').write(base64.b64decode(data))
def dialog():
    return fr('({open: d.getElementById("dlg").open, eyebrow: d.getElementById("dlg-eyebrow").textContent,'
              ' title: d.getElementById("dlg-title").textContent,'
              ' ops: [...d.querySelectorAll("#dlg-ops li")].map(l => l.textContent.replace(/\\s+/g, " ").trim()),'
              ' applyDisabled: d.getElementById("dlg-apply").disabled, applyHidden: d.getElementById("dlg-apply").hidden,'
              ' result: d.getElementById("dlg-result").hidden ? null : d.getElementById("dlg-result").className + ": " + d.getElementById("dlg-result").textContent})')
def wait_result(limit=90):
    for _ in range(limit):
        if fr('!d.getElementById("dlg-result").hidden'): return
        time.sleep(1)
    raise SystemExit('the dialog never reported a result')

print('eyebrow:', fr('d.getElementById("host").textContent'))
print('tabs:', fr('[...d.querySelectorAll("[data-tab]")].map(p=>p.textContent.trim()).join(" | ")'))
print('banner:', fr('d.getElementById("banner").hidden ? null : d.getElementById("banner").textContent'))
print('installed:', rows('installed'))

# A plain letter typed into the filter must reach it, not the app's R shortcut.
fr('(d.getElementById("q-installed").focus(), 1)')
for ch in 'gcc':
    call('Input.dispatchKeyEvent', type='keyDown', key=ch, text=ch)
    call('Input.dispatchKeyEvent', type='keyUp', key=ch)
print('typed into filter:', fr('d.getElementById("q-installed").value'))
for ch in 'r':
    call('Input.dispatchKeyEvent', type='keyDown', key=ch, text=ch)
    call('Input.dispatchKeyEvent', type='keyUp', key=ch)
print('then an r:', fr('d.getElementById("q-installed").value'))
typ('q-installed', 'gcc')
print('filtered:', rows('installed'))
typ('q-installed', '')

click('tr[data-pkg="org.gnu.gcc-libgcc"]', 2.5)
print('detail:', fr('({name: d.getElementById("d-name").textContent, desc: d.getElementById("d-desc").textContent,'
                    ' facts: [...d.querySelectorAll("#d-facts dt")].map(t=>t.textContent).join(",")})'))
fr('(d.getElementById("d-files-box").open = true, 1)'); time.sleep(2.5)
print('files:', fr('d.getElementById("d-files-sum").textContent'), '|', fr('d.getElementById("d-files").textContent.split("\\n")[0]'))
shot('installed')

click('[data-tab="updates"]', 3)
print('updates:', rows('updates'), '|', fr('d.getElementById("empty-updates").hidden ? null : d.getElementById("empty-updates").textContent'))

click('[data-tab="repos"]')
print('repos:', rows('repos'))

click('[data-tab="available"]')
typ('q-available', 'zlib-ng', 3)
print('available:', rows('available'))
shot('available')

click(f'tr[data-pkg="{PKG}"] button', 6)
print('install plan:', dialog())
shot('plan')
click('#dlg-apply', 1); wait_result()
print('install done:', dialog())
print('log tail:', fr('d.getElementById("dlg-log").textContent.trim().split("\\n").slice(-2)'))
shot('applied')
click('#dlg-cancel', 5)
print('after install, available:', [r for r in rows('available') if PKG in r])
click('[data-tab="installed"]')
print('installed count:', fr('d.getElementById("n-installed").textContent'))

# glibc is depended on: the plain plan must fail legibly, --cascade must plan.
click('tr[data-pkg="org.gnu.glibc"]', 2.5)
click('#b-uninstall', 5)
print('uninstall glibc:', dialog())
click('#dlg-cascade', 6)
print('with --cascade:', dialog())
shot('cascade')
click('#dlg-cancel', 4)

click('[data-tab="history"]')
print('history:', rows('history'))
click('#b-undo', 6)
print('undo plan:', dialog())
click('#dlg-apply', 1); wait_result()
print('undo done:', dialog())
click('#dlg-cancel', 5)
print('history after undo:', rows('history'))
print('installed after undo:', fr('d.getElementById("n-installed").textContent'))
print('console:', logs)
