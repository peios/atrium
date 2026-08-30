import base64, json
call('Emulation.setDeviceMetricsOverride', width=1500, height=980, deviceScaleFactor=1, mobile=False)
for _ in range(20):
    if ev('[...document.querySelectorAll(".tbA-tile")].some(t=>t.dataset.id==="org.peios.registry")'): break
    time.sleep(0.5)
ev('[...document.querySelectorAll(".tbA-tile")].find(t=>t.dataset.id==="org.peios.registry").click()'); time.sleep(3)
F='document.querySelector("#ws-frames iframe")'
def fr(js): return ev(f'(()=>{{ const d={F}.contentDocument; return ({js}); }})()')
def click(sel, wait=1.2):
    fr(f'(d.querySelector({json.dumps(sel)}).click(), 1)'); time.sleep(wait)
click('.trow[data-path="Machine"]')
click('.trow[data-path="Machine\\\\Software"]')
click('.trow[data-path="Machine\\\\Software\\\\Peios"]')
click('.trow[data-path="Machine\\\\Software\\\\Peios\\\\Atrium"]', 1.6)
click('.vrow[data-name="PinnedApps"]', 1.6)
shot = call('Page.captureScreenshot', format='png')['data']
open('/home/jack/.claude/jobs/771f5226/tmp/registry.png','wb').write(base64.b64decode(shot))
print('saved')
