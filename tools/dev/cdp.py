import json, os, socket, struct, sys, time, urllib.request, base64
def ws_connect(url):
    host, port, path = url.split('/')[2].split(':')[0], int(url.split('/')[2].split(':')[1]), '/' + '/'.join(url.split('/')[3:])
    s = socket.create_connection((host, port)); key = base64.b64encode(os.urandom(16)).decode()
    s.sendall(f"GET {path} HTTP/1.1\r\nHost: {host}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n".encode())
    h = b''
    while b'\r\n\r\n' not in h: h += s.recv(1)
    assert b' 101 ' in h, h; return s
def send(s, obj):
    d = json.dumps(obj).encode(); mask = os.urandom(4)
    if len(d) < 126: hdr = bytes([0x81, 0x80 | len(d)])
    elif len(d) < 65536: hdr = bytes([0x81, 0x80 | 126]) + struct.pack('>H', len(d))
    else: hdr = bytes([0x81, 0x80 | 127]) + struct.pack('>Q', len(d))
    s.sendall(hdr + mask + bytes(b ^ mask[i % 4] for i, b in enumerate(d)))
def recv(s):
    h = s.recv(2, socket.MSG_WAITALL); n = h[1] & 0x7f
    if n == 126: n = struct.unpack('>H', s.recv(2, socket.MSG_WAITALL))[0]
    elif n == 127: n = struct.unpack('>Q', s.recv(8, socket.MSG_WAITALL))[0]
    return json.loads(s.recv(n, socket.MSG_WAITALL))
cookie, url = sys.argv[1], sys.argv[2]
tab = json.load(urllib.request.urlopen(urllib.request.Request('http://127.0.0.1:9222/json/new?about:blank', method='PUT')))
s = ws_connect(tab['webSocketDebuggerUrl']); s.settimeout(15)
nid = 0
def call(method, **params):
    global nid; nid += 1; send(s, {'id': nid, 'method': method, 'params': params})
    while True:
        m = recv(s)
        if m.get('id') == nid: return m.get('result', m)
        note(m)
logs = []
def note(m):
    if m.get('method') == 'Runtime.consoleAPICalled':
        logs.append('console.%s: %s' % (m['params']['type'], ' '.join(str(a.get('value', a.get('description'))) for a in m['params']['args'])))
    elif m.get('method') == 'Runtime.exceptionThrown':
        d = m['params']['exceptionDetails']; logs.append('EXCEPTION: %s @%s:%s' % (d.get('exception', {}).get('description', d.get('text')), d.get('url'), d.get('lineNumber')))
call('Runtime.enable'); call('Network.enable')
call('Network.setCookie', name='atrium', value=cookie, url=url)
call('Page.enable'); call('Page.navigate', url=url); time.sleep(2.5)
def ev(expr):
    r = call('Runtime.evaluate', expression=expr, returnByValue=True, awaitPromise=True)
    return r.get('result', {}).get('value', r)
print('title:', ev('document.title'))
print('tiles:', ev('document.querySelectorAll(".tbA-tile").length'))
print('ws state:', ev('typeof session !== "undefined" ? (session.sock && session.sock.readyState) : "no session"'))
print('click about:', ev('(() => { const t=[...document.querySelectorAll(".tbA-tile")].find(t=>t.dataset.id==="org.peios.about"); if(!t) return "no tile"; t.click(); return "clicked"; })()'))
time.sleep(1.5)
print('after: ws hidden=', ev('document.getElementById("ws").hidden'), 'toolbox hidden=', ev('document.getElementById("toolbox").hidden'), 'frames=', ev('document.querySelectorAll("#ws-frames iframe").length'), 'tabs=', ev('document.getElementById("ws-tabs").children.length'))
# drain pending events
s.settimeout(0.5)
try:
    while True: note(recv(s))
except Exception: pass
print('\n'.join(logs) if logs else '(no console output)')
