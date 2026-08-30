import json, os, socket, struct, subprocess, sys, array
server_bin, session_bin = sys.argv[1], sys.argv[2]
a, b = socket.socketpair(socket.AF_UNIX, socket.SOCK_STREAM)
p = subprocess.Popen([server_bin], pass_fds=[b.fileno(), 3], preexec_fn=lambda: os.dup2(b.fileno(), 3))
b.close()
def rd():
    n = struct.unpack('<I', a.recv(4, socket.MSG_WAITALL))[0]
    return json.loads(a.recv(n, socket.MSG_WAITALL))
def wr(o, fd=None):
    d = json.dumps(o).encode()
    if fd is None: a.sendall(struct.pack('<I', len(d)) + d)
    else:
        a.sendmsg([struct.pack('<I', len(d))], [(socket.SOL_SOCKET, socket.SCM_RIGHTS, array.array('i', [fd]))])
        a.sendall(d)
users = {}; sessions = {}
while True:
    try: r = rd()
    except Exception: break
    t = r['type']
    if t == 'logon_start':
        users[r['conv']] = r['username']
        wr({'type':'prompt','conv':r['conv'],'messages':[],'prompts':[{'credential_ref':7,'credential_type':'password','name':'Password'}]})
    elif t == 'logon_answer':
        if any(x['credential_ref']==7 and x['data']=='secret' for x in r['answers']):
            sid = 1000 + len(sessions)
            s_end, srv_end = socket.socketpair(socket.AF_UNIX, socket.SOCK_STREAM)
            env = {'USER': users[r['conv']], 'ATRIUM_DISPLAY_NAME': 'Jack P', 'ATRIUM_SESSION': str(sid), 'ATRIUM_APPS_DIR': os.environ.get('ATRIUM_APPS_DIR',''), 'HOME': '/', 'PATH': os.environ.get('PATH', '/usr/bin'), 'LD_LIBRARY_PATH': os.environ.get('LD_LIBRARY_PATH',''), 'FAKE_SVCTL_STATE': os.environ.get('FAKE_SVCTL_STATE','')}
            sp = subprocess.Popen([session_bin], pass_fds=[s_end.fileno(), 3], preexec_fn=lambda: os.dup2(s_end.fileno(), 3), env=env)
            s_end.close(); sessions[sid] = sp
            wr({'type':'granted','conv':r['conv'],'session':sid,'username':users[r['conv']],'display_name':'Jack P'}, srv_end.fileno())
            srv_end.close()
        else: wr({'type':'denied','conv':r['conv'],'retryable':True,'reason':'Incorrect username or password'})
    elif t == 'logout':
        sp = sessions.pop(r['session'], None); wr({'type':'ok'})
        if sp: print('fake: session exit', sp.wait(timeout=5), flush=True)
    else: wr({'type':'ok'})
p.terminate()
