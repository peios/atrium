ev('[...document.querySelectorAll(".tbA-tile")].find(t=>t.dataset.id==="org.peios.about").click()'); time.sleep(1)
# bar layout: title before lights, min before close
print('bar order:', ev('[...document.getElementById("win-bar").children].map(c=>c.id||c.className)'))
print('lights order:', ev('[...document.querySelectorAll("#win-bar .win-light")].map(b=>b.id)'))
print('title text:', ev('document.getElementById("win-title").textContent'))
ev('document.getElementById("win-min").click()'); time.sleep(0.8)
print('minimised -> ws.hidden=', ev('document.getElementById("ws").hidden'), 'toolbox.hidden=', ev('document.getElementById("toolbox").hidden'), 'toolbox entry active=', ev('document.getElementById("nav-toolbox").classList.contains("is-active")'))
ev('[...document.querySelectorAll("#nav-pinned .nav-item")][0].click()'); time.sleep(0.8)
print('restored -> ws.hidden=', ev('document.getElementById("ws").hidden'), 'title=', ev('document.getElementById("win-title").textContent'))
print('atrium global:', ev('typeof window.atrium'))
s.settimeout(0.4)
try:
    while True: note(recv(s))
except Exception: pass
print('\n'.join(logs) if logs else '(no console output)')
