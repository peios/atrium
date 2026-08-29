import socket, os, base64, json, struct, sys
def connect(cookie):
    s = socket.create_connection(('127.0.0.1', 18080))
    key = base64.b64encode(os.urandom(16)).decode()
    s.sendall((f"GET /ws HTTP/1.1\r\nHost: x\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\nCookie: atrium={cookie}\r\n\r\n").encode())
    head = b''
    while b'\r\n\r\n' not in head: head += s.recv(1)
    assert b' 101 ' in head.split(b'\r\n')[0], head
    return s
def send(s, obj):
    d = json.dumps(obj).encode(); mask = os.urandom(4)
    s.sendall(bytes([0x81, 0x80 | len(d)]) + mask + bytes(b ^ mask[i % 4] for i, b in enumerate(d)))
def recv(s):
    h = s.recv(2, socket.MSG_WAITALL); n = h[1] & 0x7f
    if n == 126: n = struct.unpack('>H', s.recv(2, socket.MSG_WAITALL))[0]
    return json.loads(s.recv(n, socket.MSG_WAITALL))
if __name__ == "__main__":
  cookie = sys.argv[1]
  a = connect(cookie); print('A snapshot:', recv(a))
  send(a, {'t': 'launch', 'app': 'org.peios.about'}); print('A opened:', recv(a)); print('A focus:', recv(a))
  b = connect(cookie); print('B snapshot:', recv(b))
  send(b, {'t': 'launch', 'app': 'org.peios.nope'}); print('B error:', recv(b))
  send(a, {'t': 'close', 'id': 1}); print('A closed:', recv(a)); print('A focus:', recv(a)); print('B closed:', recv(b)); print('B focus:', recv(b))
