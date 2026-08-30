ev('[...document.querySelectorAll(".tbA-tile")].find(t=>t.dataset.id==="org.peios.terminal").click()'); time.sleep(3)
F='document.querySelector("#ws-frames iframe")'
def fr(js): return ev(f'(()=>{{ const w={F}.contentWindow, d={F}.contentDocument; return ({js}); }})()')
print('xterm rows rendered:', fr('d.querySelectorAll(".xterm-rows > div").length'))
# type a command through xterm's onData path via the pty: use the app's shell handle? Simpler: dispatch keystrokes to the terminal textarea
ev(f'(()=>{{ const d={F}.contentDocument; const ta=d.querySelector(".xterm-helper-textarea"); ta.focus(); }})()')
def type_text(txt):
    for ch in txt:
        key = {'\\n':'Enter'}.get(ch, ch)
        call('Input.dispatchKeyEvent', type='keyDown', text=ch if ch != '\n' else '\r', key='Enter' if ch=='\n' else ch)
        call('Input.dispatchKeyEvent', type='keyUp', key='Enter' if ch=='\n' else ch)
type_text('echo hi-$((2+3))\n'); time.sleep(1.2)
buf = fr('[...d.querySelectorAll(".xterm-rows > div")].map(r=>r.textContent).join("\\n")')
print('screen contains hi-5:', 'hi-5' in buf)
print('screen sample:', [l for l in buf.split('\n') if l.strip()][:4])
# throughput probe: seq 20000 through the bus, timed
import time as _t
t0=_t.time(); type_text('seq 20000 | tail -1\n')
for _ in range(60):
    _t.sleep(0.25)
    buf = fr('[...d.querySelectorAll(".xterm-rows > div")].map(r=>r.textContent).join("")')
    if '20000' in buf and '$' in buf.split('20000')[-1]: break
print('seq 20000 round trip: %.2fs' % (_t.time()-t0))
type_text('exit\n'); time.sleep(1.2)
print('exited banner:', fr('d.body.classList.contains("exited")'))
s.settimeout(0.4)
try:
    while True: note(recv(s))
except Exception: pass
print('errors:', [l for l in logs if 'EXCEPTION' in l] or 'none')
