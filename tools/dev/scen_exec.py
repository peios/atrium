ev('[...document.querySelectorAll(".tbA-tile")].find(t=>t.dataset.id==="org.peios.exectest").click()'); time.sleep(2)
F='document.querySelector("#ws-frames iframe")'
def fr(js): return ev(f'(()=>{{ const w={F}.contentWindow, d={F}.contentDocument; return ({js}); }})()')
def click(bid, wait=1.2):
    ev(f'(()=>{{ const d={F}.contentDocument; d.getElementById("{bid}").click(); }})()'); time.sleep(wait)
    return fr('({out: d.getElementById("out").textContent, exit: d.getElementById("out").dataset.exit})')
print('echo:', click('b1'))
print('seq:', click('b2'))
print('exit3:', click('b3'))
print('denied:', click('b4'))
r = call('Runtime.evaluate', expression=f'{F}.contentWindow.runjs()', returnByValue=True, awaitPromise=True)['result'].get('value')
print('js api (stdout+stderr):', r)
s.settimeout(0.4)
try:
    while True: note(recv(s))
except Exception: pass
print('errors:', [l for l in logs if 'EXCEPTION' in l] or 'none')
