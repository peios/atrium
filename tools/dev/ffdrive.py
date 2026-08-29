import json, sys, time, urllib.request
cookie, url = sys.argv[1], sys.argv[2]
B = 'http://127.0.0.1:4444'
def req(method, path, body=None):
    r = urllib.request.Request(B + path, data=json.dumps(body).encode() if body is not None else None, method=method, headers={'content-type': 'application/json'})
    return json.load(urllib.request.urlopen(r))['value']
sid = req('POST', '/session', {'capabilities': {'alwaysMatch': {'moz:firefoxOptions': {'args': ['-headless']}}}})['sessionId']
S = '/session/' + sid
req('POST', S + '/url', {'url': url})   # a first load, to own the origin for the cookie
req('POST', S + '/cookie', {'cookie': {'name': 'atrium', 'value': cookie, 'path': '/'}})
req('POST', S + '/url', {'url': url}); time.sleep(2.5)
def ev(js): return req('POST', S + '/execute/sync', {'script': js, 'args': []})
print('title:', ev('return document.title'))
print('tiles:', ev('return document.querySelectorAll(".tbA-tile").length'))
print('click:', ev('const t=[...document.querySelectorAll(".tbA-tile")].find(t=>t.dataset.id==="org.peios.about"); if(!t) return "no tile"; t.click(); return "clicked"'))
time.sleep(1.5)
print('after: ws hidden=', ev('return document.getElementById("ws").hidden'), 'toolbox hidden=', ev('return document.getElementById("toolbox").hidden'), 'frames=', ev('return document.querySelectorAll("#ws-frames iframe").length'), 'frame src=', ev('const f=document.querySelector("#ws-frames iframe"); return f && f.src'))
print('frame body:', ev('const f=document.querySelector("#ws-frames iframe"); try { return f && f.contentDocument && f.contentDocument.body && f.contentDocument.body.innerText } catch(e) { return "ERR "+e }'))
req('DELETE', S)
