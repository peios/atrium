import base64
call('Emulation.setDeviceMetricsOverride', width=1440, height=960, deviceScaleFactor=1, mobile=False)
for _ in range(20):
    if ev('[...document.querySelectorAll(".tbA-tile")].some(t=>t.dataset.id==="org.peios.services")'): break
    time.sleep(0.5)
ev('[...document.querySelectorAll(".tbA-tile")].find(t=>t.dataset.id==="org.peios.services").click()')
time.sleep(3)
F='document.querySelector("#ws-frames iframe")'
ev(f'(()=>{{ const d={F}.contentDocument; d.querySelector(`tr[data-service="authd"]`).click(); }})()')
time.sleep(2)
shot = call('Page.captureScreenshot', format='png')['data']
open('/home/jack/.claude/jobs/771f5226/tmp/services.png','wb').write(base64.b64decode(shot))
print('saved')
