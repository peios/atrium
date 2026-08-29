# A fake jobs manager: enough of PSPU §7 to exercise atriumd's client.
import json, os, socket, sys, array, signal, subprocess, threading, time
path = sys.argv[1]
try: os.unlink(path)
except FileNotFoundError: pass
srv = socket.socket(socket.AF_UNIX, socket.SOCK_SEQPACKET); srv.bind(path); srv.listen(4)
jobs = {}
def view(j):
    p = j['proc']; rc = p.poll()
    st = 'running' if rc is None else ('completed' if rc == 0 else 'failed')
    return {'id': j['id'], 'type': 'submitted', 'state': st, 'cause': None if rc is None else ('explicit_stop' if j.get('stopped') else None),
            'submitter': 'S-1-5-18', 'identity': 'S-1-5-18', 'logon_session': 999, 'description': j['desc'], 'image_path': j['img'],
            'pid': p.pid if rc is None else None, 'ready': None, 'exit_code': rc if (rc is not None and rc >= 0) else None,
            'exit_signal': -rc if (rc is not None and rc < 0) else None, 'status_text': None, 'progress': None,
            'created_at': 't', 'started_at': 't', 'ended_at': None if rc is None else 't'}
def serve(c):
    while True:
        try: data, anc, _, _ = c.recvmsg(1 << 20, socket.CMSG_SPACE(64 * 4))
        except OSError: break
        if not data: break
        fds = []
        for lvl, typ, d in anc:
            if lvl == socket.SOL_SOCKET and typ == socket.SCM_RIGHTS:
                a = array.array('i'); a.frombytes(d[:len(d) - len(d) % 4]); fds += list(a)
        r = json.loads(data); cmd = r['command']
        if cmd == 'submit':
            names = r.get('descriptors', [])
            assert len(names) == len(fds), (names, fds)
            jid = 'job-%d' % (len(jobs) + 1)
            env = dict(r.get('environment', {})); env['LISTEN_FDS'] = str(len(fds)); env['LISTEN_FDNAMES'] = ':'.join(names)
            def pre():
                for i, fd in enumerate(fds): os.dup2(fd, 3 + i)
            p = subprocess.Popen([r['image_path']] + r.get('arguments', []), env=env, cwd=r.get('working_directory', '/'),
                                 pass_fds=[3 + i for i in range(len(fds))] + fds, preexec_fn=pre)
            for fd in fds: os.close(fd)
            jobs[jid] = {'id': jid, 'proc': p, 'desc': r.get('description', ''), 'img': r['image_path']}
            time.sleep(0.05)
            resp = json.dumps({'status': 'ok', 'job': view(jobs[jid])}).encode()
            pidfd = os.pidfd_open(p.pid)
            c.sendmsg([resp], [(socket.SOL_SOCKET, socket.SCM_RIGHTS, array.array('i', [pidfd]))]); os.close(pidfd)
        elif cmd in ('status', 'stop'):
            j = jobs.get(r['job_id'])
            if not j: c.send(json.dumps({'status': 'error', 'code': 'UNKNOWN_JOB', 'message': 'no such job'}).encode()); continue
            if cmd == 'stop':
                j['stopped'] = True; j['proc'].terminate()
                try: j['proc'].wait(timeout=r.get('stop_timeout', 2))
                except subprocess.TimeoutExpired: j['proc'].kill(); j['proc'].wait()
            c.send(json.dumps({'status': 'ok', 'job': view(j)}).encode())
        else:
            c.send(json.dumps({'status': 'error', 'code': 'INVALID_ARGUMENTS', 'message': 'unknown command'}).encode())
    c.close()
print('fake peinit listening', flush=True)
while True:
    c, _ = srv.accept(); threading.Thread(target=serve, args=(c,), daemon=True).start()
