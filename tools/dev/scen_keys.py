kd = lambda tgt, opts: f'(() => {{ {tgt}.dispatchEvent(new KeyboardEvent("keydown", {{bubbles:true, cancelable:true, {opts}}})); }})()'
ev('[...document.querySelectorAll(".tbA-tile")].find(t=>t.dataset.id==="org.peios.about").click()'); time.sleep(2)
print('keymap size:', ev('window.atrium.session.keymap.length'))
print('open, ws.hidden:', ev('document.getElementById("ws").hidden'))
# Alt+M in the SHELL document -> minimise
ev(kd('window', 'key:"m", altKey:true')); time.sleep(0.8)
print('after Alt+M (shell): toolbox visible=', ev('!document.getElementById("toolbox").hidden'))
# restore via Alt+1
ev(kd('window', 'key:"1", altKey:true')); time.sleep(0.8)
print('after Alt+1: ws visible=', ev('!document.getElementById("ws").hidden'))
# Alt+M dispatched INSIDE the app frame -> SDK forwards -> minimise
ev(kd('document.querySelector("#ws-frames iframe").contentWindow', 'key:"m", altKey:true')); time.sleep(0.8)
print('after Alt+M (in frame): toolbox visible=', ev('!document.getElementById("toolbox").hidden'))
ev(kd('window', 'key:"1", altKey:true')); time.sleep(0.8)
# local shortcut R inside frame: change sub text sentinel then reload proves re-render
ev('document.querySelector("#ws-frames iframe").contentDocument.getElementById("sub").textContent = "stale"')
ev(kd('document.querySelector("#ws-frames iframe").contentWindow', 'key:"r"')); time.sleep(1)
print('after R (in frame): sub=', ev('document.querySelector("#ws-frames iframe").contentDocument.getElementById("sub").textContent.slice(0,14)'))
# Ctrl+K -> toolbox + search focused
ev(kd('window', 'key:"k", ctrlKey:true')); time.sleep(0.5)
print('after Ctrl+K: toolbox visible=', ev('!document.getElementById("toolbox").hidden'), 'search focused=', ev('document.activeElement === document.getElementById("filter")'))
# Alt+W closes the (still open) window: first go back to it
ev(kd('window', 'key:"1", altKey:true')); time.sleep(0.5)
ev(kd('window', 'key:"w", altKey:true')); time.sleep(0.8)
print('after Alt+W: frames=', ev('document.querySelectorAll("#ws-frames iframe").length'))
s.settimeout(0.5)
try:
    while True: note(recv(s))
except Exception: pass
errs=[l for l in logs if 'EXCEPTION' in l or 'unknown action' in l]
print('errors:', errs or 'none')
