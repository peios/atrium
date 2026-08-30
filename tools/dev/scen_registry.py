# The Registry app against fakebin/reg. Needs FAKE_REG_STATE shared with
# the session process so assertions and mutations line up.
import json, os

for _ in range(20):
    if ev('[...document.querySelectorAll(".tbA-tile")].some(t=>t.dataset.id==="org.peios.registry")'): break
    time.sleep(0.5)
else:
    raise SystemExit('registry tile never appeared')
ev('[...document.querySelectorAll(".tbA-tile")].find(t=>t.dataset.id==="org.peios.registry").click()'); time.sleep(3)

F = 'document.querySelector("#ws-frames iframe")'
def fr(js): return ev(f'(()=>{{ const d={F}.contentDocument, w={F}.contentWindow; return ({js}); }})()')
def click(sel, wait=1.2):
    fr(f'(d.querySelector({json.dumps(sel)}).click(), 1)'); time.sleep(wait)

# Tree: hives present; expand down to Atrium.
print('hives:', fr('[...d.querySelectorAll(".trow")].map(r=>r.dataset.path).join(",")'))
click('.trow[data-path="Machine"]')
click('.trow[data-path="Machine\\\\Software"]')
click('.trow[data-path="Machine\\\\Software\\\\Peios"]')
click('.trow[data-path="Machine\\\\Software\\\\Peios\\\\Atrium"]', 1.6)
print('crumb:', fr('d.getElementById("crumb").textContent'))
print('values:', fr('[...d.querySelectorAll(".vrow")].map(r=>r.dataset.name || "(default)").join(",")'))
print('theme row:', fr('(r=>r && r.textContent.replace(/\\s+/g," ").trim())(d.querySelector(`.vrow[data-name="Theme"]`))'))

# Inspect Theme: provenance layer + seq.
click('.vrow[data-name="Theme"]', 1.6)
print('prov:', fr('d.getElementById("prov")?.textContent'))
print('layers:', fr('[...d.querySelectorAll(".lrow")].map(r=>r.dataset.layer).join(",")'))

# Edit Theme (CAS via --expected-seq).
click('#b-edit', 0.6)
fr('(d.getElementById("ed-data").value = "light", 1)')
click('#ed-save', 1.8)
print('after edit:', fr('(r=>r.textContent.replace(/\\s+/g," ").trim())(d.querySelector(`.vrow[data-name="Theme"]`))'))
STATE = os.environ.get('FAKE_REG_STATE', '/tmp/fake-reg.json')
st = json.load(open(STATE))
print('on disk:', st['keys']['Machine\\Software\\Peios\\Atrium']['values']['Theme'])

# Stale-sequence conflict: open the editor (snapshotting provenance),
# THEN bump seq behind the app's back, then save.
click('.vrow[data-name="Theme"]', 1.4)
click('#b-edit', 0.6)
st = json.load(open(STATE))
st['keys']['Machine\\Software\\Peios\\Atrium']['values']['Theme']['seq'] = 99
json.dump(st, open(STATE, 'w'))
fr('(d.getElementById("ed-data").value = "blue", 1)')
click('#ed-save', 1.8)
print('conflict:', fr('d.getElementById("ed-err")?.textContent'))

# New value, then delete it (two-click arm).
click('#b-newval', 0.5)
fr('(d.getElementById("nv-name").value = "TestVal", 1)')
fr('(d.getElementById("nv-data").value = "hello", 1)')
click('#nv-save', 1.8)
print('after create:', fr('[...d.querySelectorAll(".vrow")].map(r=>r.dataset.name).includes("TestVal")'))
click('.vrow[data-name="TestVal"] .ibtn', 0.4)
click('.vrow[data-name="TestVal"] .ibtn', 1.8)
print('after delete:', fr('[...d.querySelectorAll(".vrow")].map(r=>r.dataset.name).includes("TestVal")'))

# New subkey.
click('#b-newkey', 0.5)
fr('(d.getElementById("nk-name").value = "Sub1", 1)')
click('#nk-save', 1.8)
print('subkey visible:', fr(f'!!d.querySelector({json.dumps(chr(46)+"trow[data-path="+json.dumps("Machine\\Software\\Peios\\Atrium\\Sub1")+"]")})'))

# Security tab: SDDL parsed.
click('.tab[data-tab="security"]', 1.6)
print('sddl:', fr('d.getElementById("sddl")?.textContent'))
print('aces:', fr('d.querySelectorAll(".ace").length'))

# Docs: value manual card in the inspector, key card in the Docs tab.
click('.tab[data-tab="values"]', 0.8)
click('.vrow[data-name="Theme"]', 1.6)
print('value manual:', fr('(d.getElementById("insp-docs")?.textContent || "").includes("colour scheme")'))
click('.vrow[data-name="SessionKey"]', 1.6)
print('undocumented value has no card:', fr('!d.getElementById("insp-docs")'))
click('.tab[data-tab="docs"]', 1.6)
print('key docs:', fr('(d.getElementById("docs")?.textContent || "").slice(0, 40)'))
click('.trow[data-path="CurrentUser"]', 1.6)
print('no manual entry:', fr('(d.querySelector(".detail-body .dstate")?.textContent || "").slice(0, 30)'))
click('.trow[data-path="Machine\\Software\\Peios\\Atrium"]', 1.2)
click('.tab[data-tab="values"]', 0.8)

# Denied key.
click('.tab[data-tab="values"]', 0.5)
click('.trow[data-path="Machine\\\\System"]')
click('.trow[data-path="Machine\\\\System\\\\Secrets"]', 1.6)
print('denied state:', fr('(d.querySelector(".dstate.denied")||{}).textContent?.slice(0,30)'))
print('denied badge:', fr(f'!!d.querySelector({json.dumps(chr(46)+"trow[data-path="+json.dumps("Machine\\System\\Secrets")+"] .tbadge.lock")})'))

# Fuzzy search over loaded keys.
fr('(d.getElementById("q").value = "atrium", d.getElementById("q").dispatchEvent(new Event("input")), 1)')
print('search hits:', fr('[...d.querySelectorAll(".trow")].map(r=>r.dataset.path).join(",")'))

print('console errors:', [l for l in logs if 'EXCEPTION' in l or 'error' in l.lower()][:5] or 'none')
