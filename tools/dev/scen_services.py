# The Services app against fakebin/svctl. Needs FAKE_SVCTL_STATE in the
# environment (shared with the session process) so mode flips reach the fake.
import json, os

STATE = os.environ.get('FAKE_SVCTL_STATE', '/tmp/fake-svctl.json')

def set_mode(mode):
    with open(STATE) as f: st = json.load(f)
    st['mode'] = mode
    with open(STATE, 'w') as f: json.dump(st, f)

for _ in range(20):
    if ev('[...document.querySelectorAll(".tbA-tile")].some(t=>t.dataset.id==="org.peios.services")'): break
    time.sleep(0.5)
else:
    raise SystemExit('services tile never appeared')
ev('[...document.querySelectorAll(".tbA-tile")].find(t=>t.dataset.id==="org.peios.services").click()'); time.sleep(3)
F = 'document.querySelector("#ws-frames iframe")'
def fr(js): return ev(f'(()=>{{ const d={F}.contentDocument; return ({js}); }})()')
def clickrow(name, wait=1.5):
    fr(f'(d.querySelector(`tr[data-service="{name}"]`).click(), 1)'); time.sleep(wait)
def button(bid, wait=2.0):
    fr(f'(d.getElementById("{bid}").click(), 1)'); time.sleep(wait)
def detail():
    return fr('({name: d.getElementById("d-name").textContent,'
              ' state: [...d.querySelectorAll("#d-facts dd")][0]?.textContent,'
              ' startDis: d.getElementById("b-start").disabled,'
              ' stopDis: d.getElementById("b-stop").disabled,'
              ' resetHid: d.getElementById("b-reset").hidden,'
              ' err: d.getElementById("action-err").hidden ? null : d.getElementById("action-err").textContent})')

print('rows:', fr('[...d.querySelectorAll("tbody tr")].map(r => r.dataset.service).join(",")'))
print('eventd dot:', fr('d.querySelector(`tr[data-service="eventd"] .dot`).className'))
print('authd cell:', fr('d.querySelector(`tr[data-service="authd"] .name`).textContent'))

clickrow('authd')
print('detail(authd):', detail())
print('facts:', fr('[...d.querySelectorAll("#d-facts dt")].map(t=>t.textContent).join(",")'))

button('b-stop', 3)
print('after stop:', detail())
button('b-start', 3)
print('after start:', detail())

clickrow('eventd')
print('detail(eventd):', detail())
button('b-reset', 3)
print('after reset:', detail())

set_mode('denied')
clickrow('authd')
button('b-stop', 3)
print('denied:', detail())

set_mode('noconnect'); time.sleep(5)
print('banner:', fr('d.getElementById("banner").hidden ? null : d.getElementById("banner").textContent.slice(0, 60)'))
set_mode('ok'); time.sleep(12)
print('banner cleared:', fr('d.getElementById("banner").hidden'))
print('rows again:', fr('d.querySelectorAll("tbody tr").length'))

print('console errors:', [l for l in logs if 'EXCEPTION' in l or 'error' in l.lower()][:5] or 'none')
