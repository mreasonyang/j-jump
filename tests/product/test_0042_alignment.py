"""Origin repair parity on isolated state, real terminal input and local transports."""
import contextlib, errno, hashlib, json, os, pathlib, pty, re, select, shlex, shutil, signal, socket, sqlite3, stat, struct, subprocess, sys, tempfile, threading, time, unittest
ROOT=pathlib.Path(__file__).resolve().parents[2]
BIN=pathlib.Path(os.environ.get('JJ_TEST_BIN',ROOT/'target/debug/jjump')).resolve()
class Terminal:
    def __init__(self,argv,env,cwd):
        self.pid,self.fd=pty.fork(); self.output=b''; self.cursor=0
        if self.pid==0:
            os.chdir(cwd); os.execvpe(argv[0],argv,dict(env,PWD=str(cwd)))
    def send(self,text): os.write(self.fd,text.encode())
    def drain(self,seconds=.05):
        end=time.monotonic()+seconds
        while time.monotonic()<end:
            if select.select([self.fd],[],[],.02)[0]:
                try: data=os.read(self.fd,65536)
                except OSError as e:
                    if e.errno==errno.EIO: break
                    raise
                if not data: break
                self.output+=data
    def until(self,needle,timeout=8):
        end=time.monotonic()+timeout
        while self.output.find(needle,self.cursor)<0 and time.monotonic()<end: self.drain()
        at=self.output.find(needle,self.cursor)
        if at<0: raise AssertionError((needle,self.output[-2000:]))
        self.cursor=at+len(needle)
    def wait(self,timeout=8):
        end=time.monotonic()+timeout
        while time.monotonic()<end:
            self.drain(); pid,status=os.waitpid(self.pid,os.WNOHANG)
            if pid: self.pid=0; self.drain(); return os.waitstatus_to_exitcode(status)
        raise AssertionError(('did not exit',self.output[-2000:]))
    def close(self):
        if self.pid:
            try: os.kill(self.pid,signal.SIGKILL); os.waitpid(self.pid,0)
            except (ProcessLookupError,ChildProcessError): pass
        os.close(self.fd)
class Alignment(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory(); self.root=pathlib.Path(self.tmp.name).resolve(); self.home=self.root/'home'; self.cwd=self.home/'neutral'; self.cwd.mkdir(parents=True); self.state=self.root/'state'
        self.protocol=int(os.environ.get('JJ_FIXTURE_ADAPTER_VERSION','4'))
        self.env=dict(HOME=str(self.home),J_JUMP_HOME=str(self.state),PATH=str(BIN.parent)+':'+os.environ['PATH'],LANG='en_US.UTF-8',LC_ALL='en_US.UTF-8',J_JUMP_LANG='en',TERM='xterm',NO_COLOR='1',J_JUMP_PICKER='numbered')
        if os.environ.get('JJ_TEST_HTTPS_PROXY'): self.env['HTTPS_PROXY']=os.environ['JJ_TEST_HTTPS_PROXY']
    def tearDown(self): self.tmp.cleanup()
    def cli(self,*args,code=0,cwd=None,env=None):
        where=cwd or self.cwd; r=subprocess.run([str(BIN),*map(str,args)],env=dict(env or self.env,PWD=str(where)),cwd=where,capture_output=True,timeout=15)
        self.assertEqual(r.returncode,code,(args,r.stdout,r.stderr)); return r
    def visit(self,path,n=1):
        path.mkdir(parents=True,exist_ok=True)
        for _ in range(n): self.cli('record','--',path,cwd=path)
    def db(self): return self.state/'data/visits.db'
    @contextlib.contextmanager
    def process(self,*args):
        t=Terminal([str(BIN),*map(str,args)],self.env,self.cwd)
        try: yield t
        finally: t.close()
    def runtime(self,env=None):
        env=env or self.env; keys=('HTTPS_PROXY','https_proxy','HTTP_PROXY','http_proxy','ALL_PROXY','all_proxy','NO_PROXY','no_proxy','REQUEST_METHOD')
        data=os.fsencode(self.state/'config/config.json')+b'\0'+os.fsencode(self.state/'cache')+b'\0adapter-v'+str(self.protocol).encode()
        data+=b''.join(k.encode()+b'\0'+env.get(k,'').encode()+b'\0' for k in keys)
        return pathlib.Path('/private/tmp' if sys.platform=='darwin' else '/tmp')/('jj-'+str(os.geteuid())+'-'+hashlib.sha256(data).hexdigest()[:24])
    @staticmethod
    def recv(c,n):
        out=b''
        while len(out)<n:
            part=c.recv(n-len(out))
            if not part: raise EOFError()
            out+=part
        return out
    @contextlib.contextmanager
    def reply(self,name=None,close=False):
        path=self.runtime(); path.mkdir(mode=0o700); s=socket.socket(socket.AF_UNIX); s.bind(str(path/'jev.sock')); (path/'jev.sock').chmod(0o600); s.listen(); s.settimeout(.1); end=threading.Event(); seen=[]; errors=[]
        def work():
            while not end.is_set():
                try: c,_=s.accept()
                except socket.timeout: continue
                except OSError: break
                with c:
                    if close: continue
                    try:
                        c.settimeout(2); wire=json.loads(self.recv(c,struct.unpack('>I',self.recv(c,4))[0])); req=json.loads(wire['payload']); seen.append(req)
                        options=req['questions']['destination']['criteria']; choice='none' if name is None else next(k for k,v in options.items() if isinstance(v,dict) and v['name']==name)
                        answer=dict(type='choice',choice=choice,confidence=1.0,probabilities={k:float(k==choice) for k in options})
                        answers={k:dict(type='noul',noul=1.0) for k in req['questions'] if k!='destination'}; answers['destination']=answer
                        body=json.dumps(dict(model=req['model'],answers=answers))
                        data=json.dumps(dict(version=self.protocol,id=wire['id'],status='ok',body=body)).encode(); c.sendall(struct.pack('>I',len(data))+data)
                    except (BrokenPipeError,ConnectionResetError): pass
                    except Exception as e: errors.append(repr(e))
        thread=threading.Thread(target=work); thread.start()
        try: yield seen
        finally:
            end.set(); s.close(); thread.join(3); shutil.rmtree(path); self.assertFalse(thread.is_alive()); self.assertEqual(errors,[])
    def semantic(self):
        self.env['TYPESAFE_API_KEY']='synthetic-alignment-key'
        self.cli('config','set','semantic','on'); self.cli('config','set','consent','always')
    def package(self):
        p=self.root/'package'; p.mkdir(); shutil.copy2(BIN,p/'jjump'); shutil.copy2(os.environ.get('JJ_TEST_INSTALLER',ROOT/'packaging/install.sh'),p/'install.sh'); (p/'binary.sha256').write_text(hashlib.sha256(BIN.read_bytes()).hexdigest()+'\n'); return p
    def installer(self,package,*args):
        return subprocess.run([str(package/'install.sh'),'--prefix',str(self.home/'.local'),*args],env=self.env,capture_output=True,timeout=20)
    def test_install_first_does_not_create_linux_private_data(self):
        p=self.package(); r=self.installer(p); self.assertEqual(r.returncode,0,r.stderr)
        self.assertFalse((self.home/'.local/share/j-jump').exists())
        self.assertTrue((self.home/'.local/share/j-jump-install/installed.sha256').is_file())
        if sys.platform.startswith('linux'):
            env={k:v for k,v in self.env.items() if k!='J_JUMP_HOME'}; target=self.home/'code/acme-api'; target.mkdir(parents=True)
            self.cli('record','--',target,cwd=target,env=env)
            self.assertEqual(stat.S_IMODE((self.home/'.local/share/j-jump').stat().st_mode),0o700)
            self.assertEqual(self.cli('query','acme',env=env).stdout.strip(),os.fsencode(target)); self.cli('doctor',env=env)
    def test_same_archive_reinstall_is_noop(self):
        p=self.package(); a=self.installer(p); self.assertEqual(a.returncode,0,a.stderr); binary=self.home/'.local/bin/jjump'; before=binary.stat().st_mtime_ns
        b=self.installer(p); self.assertEqual(b.returncode,0,b.stderr); self.assertEqual(binary.stat().st_mtime_ns,before)
        self.assertFalse((binary.parent/'jjump.previous').exists())
    def test_project_ranking_and_human_explain(self):
        api=self.home/'code/acme-api'; handlers=api/'src/handlers'; self.visit(api,3); self.visit(handlers,20)
        self.assertEqual(self.cli('query','acme').stdout.strip(),os.fsencode(api)); text=self.cli('explain','acme').stdout
        self.assertIn(b'~/code/acme-api',text); self.assertIn(b'j goes here',text)
        detail=json.loads(self.cli('explain','--json','acme').stdout); self.assertEqual(detail['lexical_matches'],2)
        self.assertEqual(detail['matches'][0]['path'],str(api))
    def test_grouped_recall_keeps_rare_target_and_one_question(self):
        target=self.home/'billing-service'; self.visit(target)
        with sqlite3.connect(self.db()) as db:
            for i in range(1000):
                p=self.home/'projects'/str(i)/'build'; p.mkdir(parents=True); db.execute('INSERT INTO visits(path,key,count,last_seen,weight) VALUES(?,?,?,?,?)',(str(p),str(p).lower(),20,int(time.time()),20.0))
        req=json.loads(self.cli('preview','payment','processing').stdout); self.assertEqual(list(req['questions']),['destination'])
        names=[v['name'] for v in req['questions']['destination']['criteria'].values() if isinstance(v,dict)]
        self.assertEqual(set(names),{'billing-service','build'}); self.assertEqual(len(names),2)
        self.assertNotIn('candidates',req['state']); self.assertLessEqual(len(json.dumps(req).encode()),65536)
    def test_payload_uses_maximal_priority_prefix_within_64k(self):
        self.visit(self.home/'seed'); parent=self.home/('p'*250); parent.mkdir()
        now=int(time.time())
        with sqlite3.connect(self.db()) as db:
            db.execute('DELETE FROM visits')
            for i in range(254):
                path=parent/(f'{i:03d}'+'x'*237); path.mkdir()
                db.execute('INSERT INTO visits(path,key,count,last_seen,weight) VALUES(?,?,?,?,?)',(str(path),str(path).lower(),1,now,1.0))
        self.cli('config','set','privacy','balanced')
        result=self.cli('preview','notlexical'); request=json.loads(result.stdout); options=request['questions']['destination']['criteria']; count=len(options)-1
        self.assertGreater(count,0); self.assertLess(count,254); self.assertLessEqual(len(result.stdout.strip()),65536)
        self.assertEqual(set(options),{'none'}|{'c'+str(i) for i in range(count)})
        self.assertEqual({v['name'] for k,v in options.items() if k!='none'},{f'{i:03d}'+'x'*237 for i in range(count)})
        options['c'+str(count)]={'name':f'{count:03d}'+'x'*237,'parent':'p'*250,'usage':'low'}
        self.assertGreater(len(json.dumps(request,separators=(',',':'),ensure_ascii=False).encode()),65536)
    def test_suggestion_group_is_first_and_selection_explicit(self):
        self.visit(self.home/'high',20); self.visit(self.home/'wanted'); self.semantic()
        with self.reply('wanted') as seen, self.process('query','--interactive','notlexical') as t:
            t.until(b'Empty/q: cancel'); rows=[x for x in t.output.splitlines() if re.match(rb'\s*\d+\. ',x)]
            self.assertIn(b'wanted',rows[0]); self.assertIn(b'Jev',rows[0]); self.assertIn(b'Jev suggestion:',t.output)
            t.send('q\n'); self.assertEqual(t.wait(),130); self.assertNotIn(b'J130',t.output)
    def test_group_members_remain_separate_local_choices(self):
        self.visit(self.home/'a/shared'); self.visit(self.home/'b/shared'); self.semantic()
        # Equal visit times keep this grouping test independent of decay ranking.
        with sqlite3.connect(self.db()) as db:
            db.execute('UPDATE visits SET last_seen=?, weight=1.0', (int(time.time()),))
        with self.reply('shared'), self.process('query','--interactive','notlexical') as t:
            t.until(b'Empty/q: cancel'); self.assertEqual(t.output.count(b'[Jev]'),2); t.send('2\n'); self.assertEqual(t.wait(),0)
            self.assertTrue(t.output.rstrip().endswith(os.fsencode(self.home/'b/shared')), t.output[-2000:])
    def test_fzf_suggestion_group_is_first_and_marked(self):
        self.visit(self.home/'high',20); self.visit(self.home/'wanted'); self.semantic()
        fake=self.root/'fake'; fake.mkdir(); capture=self.root/'fzf.json'; program=fake/'fzf'
        program.write_text('#!/usr/bin/python3\nimport json,sys\nrows=sys.stdin.buffer.read().split(b"\\0")\nopen('+repr(str(capture))+',"w").write(json.dumps({"rows":[x.decode() for x in rows if x],"args":sys.argv}))\nsys.exit(130)\n'); program.chmod(0o755)
        self.env['PATH']=str(fake)+':'+self.env['PATH']; self.env['J_JUMP_PICKER']='fzf'
        with self.reply('wanted'), self.process('query','--interactive','notlexical') as t:
            self.assertEqual(t.wait(),130); self.assertNotIn(b'J130',t.output)
        data=json.loads(capture.read_text()); self.assertIn('wanted',data['rows'][0]); self.assertIn('[Jev suggestion]',data['rows'][0]); self.assertTrue(any(x.startswith('--header=Jev suggestion: wanted') for x in data['args']))
    def test_full_store_prunes_after_commit_and_retains_new_visit(self):
        self.visit(self.home/'seed')
        with sqlite3.connect(self.db()) as db: db.executemany('INSERT INTO visits(path,key,count,last_seen,weight) VALUES(?,?,?,?,?)',[(f'/synthetic/p{i}',f'/synthetic/p{i}',100,int(time.time()),100.0) for i in range(9999)])
        fresh=self.home/'fresh'; self.visit(fresh)
        with sqlite3.connect(self.db()) as db:
            self.assertEqual(db.execute('select count(*) from visits').fetchone()[0],9000)
            self.assertEqual(db.execute('select count(*) from visits where path=?',(str(fresh),)).fetchone()[0],1)
    def test_prune_failure_does_not_rollback_admission(self):
        self.visit(self.home/'seed')
        with sqlite3.connect(self.db()) as db:
            db.executemany('INSERT INTO visits(path,key,count,last_seen,weight) VALUES(?,?,?,?,?)',((str(self.home/('hot'+str(i))),str(i),100,int(time.time()),100.0) for i in range(9999)))
            db.execute("CREATE TRIGGER refuse_prune BEFORE DELETE ON visits BEGIN SELECT RAISE(FAIL,'synthetic prune failure'); END")
        arrived=self.home/'committed'; self.visit(arrived)
        with sqlite3.connect(self.db()) as db:
            self.assertEqual(db.execute('SELECT count FROM visits WHERE path=?',(str(arrived),)).fetchone()[0],1)
            self.assertEqual(db.execute('SELECT count(*) FROM visits').fetchone()[0],10001)
    def test_supervised_full_store_visits_are_persisted(self):
        self.visit(self.home/'seed')
        with sqlite3.connect(self.db()) as db:
            db.executemany('INSERT INTO visits(path,key,count,last_seen,weight) VALUES(?,?,?,?,?)',((str(self.home/('hot'+str(i))),str(i),100,int(time.time()),100.0) for i in range(9999)))
        paths=[self.home/('observed-'+str(i)) for i in range(20)]
        for path in paths:
            path.mkdir(); result=self.cli('observe',path,cwd=path); self.assertTrue(result.stdout.strip().isdigit())
            end=time.monotonic()+2
            while time.monotonic()<end:
                with sqlite3.connect(self.db()) as db: persisted=db.execute('SELECT count FROM visits WHERE path=?',(str(path),)).fetchone()
                if persisted: break
                time.sleep(.005)
            self.assertEqual(persisted,(1,),('supervisor launched but visit missing',path))
        with sqlite3.connect(self.db()) as db: self.assertLessEqual(db.execute('SELECT count(*) FROM visits').fetchone()[0],10000)
    def test_cancel_releases_request_lock_and_next_http_slot(self):
        self.visit(self.home/'alpha'); self.semantic()
        proxy=socket.socket(); proxy.bind(('127.0.0.1',0)); proxy.listen(); proxy.settimeout(.1)
        self.env['HTTPS_PROXY']='http://127.0.0.1:'+str(proxy.getsockname()[1]); end=threading.Event(); connections=[]; threads=[]
        def hold(c):
            with c:
                c.settimeout(2); c.recv(8192); end.wait(12)
        def accept():
            while not end.is_set():
                try:c,_=proxy.accept()
                except socket.timeout:continue
                except OSError:break
                connections.append(time.monotonic()); t=threading.Thread(target=hold,args=(c,)); t.start(); threads.append(t)
        listener=threading.Thread(target=accept); listener.start()
        try:
            with self.process('query','--interactive','notlexical') as first:
                first.until(b'[Enter=yes / W=wait]'); first.send('\n'); first.until(b'Empty/q: cancel'); first.send('q\n'); self.assertEqual(first.wait(),130)
            started=time.monotonic()
            with self.process('query','--interactive','notlexical') as second:
                while len(connections)<2 and time.monotonic()-started<.3: second.drain(.01)
                self.assertEqual(len(connections),2,second.output); self.assertLess(connections[1]-started,.3)
                second.until(b'Checking with Jev'); second.send('\x03'); self.assertEqual(second.wait(),130)
            with sqlite3.connect(self.state/'cache/semantic-cache.db') as db:
                self.assertEqual(db.execute('SELECT failures FROM circuit').fetchone()[0],0)
                self.assertEqual(db.execute('SELECT count(*) FROM cache').fetchone()[0],0)
                self.assertEqual(db.execute('SELECT count(*) FROM dispatch').fetchone()[0],2)
        finally:
            end.set(); proxy.close(); listener.join(3)
            for t in threads:t.join(3)
            self.cli('adapter','stop')
    def test_aged_missing_lexical_does_not_become_semantic_miss(self):
        target=self.home/'stale'; self.visit(target); target.rmdir(); self.semantic()
        with sqlite3.connect(self.db()) as db: db.execute('UPDATE visits SET last_seen=?',(int(time.time())-91*86400,))
        with self.process('query','stale') as t:
            self.assertEqual(t.wait(),6); self.assertNotIn(b'Checking with Jev',t.output)
        self.assertFalse((self.state/'cache/semantic-cache.db').exists())
    def test_busy_is_retryable_not_corrupt(self):
        self.visit(self.home/'code/alpha'); db=sqlite3.connect(self.db(),isolation_level=None); db.execute('BEGIN EXCLUSIVE')
        try: r=self.cli('query','alpha',code=7)
        finally: db.execute('ROLLBACK'); db.close()
        self.assertIn(b'busy',r.stderr); self.assertIn(b'retry',r.stderr); self.assertNotIn(b'corrupt',r.stderr)
    def test_private_state_error_names_exact_remedy(self):
        self.visit(self.home/'code/alpha'); data=self.state/'data'; data.chmod(0o755)
        r=self.cli('query','alpha',code=7); self.assertIn(os.fsencode(data),r.stderr); self.assertIn(b'chmod 700',r.stderr)
    def test_numbered_filter_reprompts_without_requests(self):
        for i in range(45): self.visit(self.home/f'proj-{i:03d}')
        with self.process('query','--interactive') as t:
            t.until(b'Empty/q: cancel'); self.assertNotIn(b'21. ',t.output); t.send('999\n'); t.until(b'Enter a number in')
            t.send('proj-023\n'); t.until(b'Matching'); t.send('1\n'); self.assertEqual(t.wait(),0); self.assertTrue(t.output.rstrip().endswith(os.fsencode(self.home/'proj-023')))
        self.assertFalse((self.state/'cache/semantic-cache.db').exists())
    def test_no_quota_and_current_state(self):
        self.cli('config','set','semantic','on'); saved=json.loads(self.cli('config','show','--json').stdout)['saved']
        self.assertNotIn('request_limit',saved); self.assertEqual(saved['schema_version'],3); self.cli('config','set','request_limit','50',code=2)
    def test_clean_init_refusal_wrong_shell_and_alias(self):
        cases=[('bash','zsh',''),('zsh','bash',''),('bash','bash','shopt -s expand_aliases; alias j=true; '),('zsh','zsh','alias j=true; ')]
        for shell,init,prefix in cases:
            with self.subTest(shell=shell,init=init,prefix=prefix):
                script=prefix+'eval "$('+shlex.quote(str(BIN))+' init '+init+')"; rc=$?; printf "RC:%s\\n" "$rc"; if typeset -f __jj_resolve >/dev/null 2>&1; then echo defined; else echo none; fi'
                r=subprocess.run([shell,'-c',script],env=self.env,capture_output=True,timeout=10)
                self.assertEqual(r.stderr.count(b'J2:'),1,r.stderr); self.assertEqual(r.stderr.strip().count(b'\n'),0,r.stderr); self.assertIn(b'RC:2',r.stdout); self.assertIn(b'none',r.stdout)
    def test_bare_command_is_help_and_setup_is_case_insensitive(self):
        with self.process() as t:
            self.assertEqual(t.wait(),0); self.assertIn(b'Usage:',t.output); self.assertNotIn(b'Semantic provider',t.output)
        with self.process('setup') as t:
            t.until(b'Semantic provider'); t.send('OFF\n'); t.until(b'Local visit tracking'); t.send('ON\n'); t.until(b'Advanced settings'); t.send('\n'); t.until(b'Saved.'); self.assertEqual(t.wait(),0)
    def test_nonsecret_network_key_choice_and_menu_ignore_case(self):
        with self.process('setup') as t:
            t.until(b'Semantic provider'); t.send('JEV\n'); t.until(b'Network permission'); t.send('ALWAYS\n'); t.until(b'Fields'); t.send('STRICT\n'); t.until(b'API key ['); t.send('SKIP\n'); t.until(b'Local visit tracking'); t.send('ON\n'); t.until(b'Advanced settings'); t.send('RESET\n'); t.until(b'Saved.'); self.assertEqual(t.wait(),0)
        config=json.loads((self.state/'config/config.json').read_text()); self.assertFalse(config['semantic']); self.assertEqual(config['schema_version'],3)
    def test_missing_history_prune_preview_apply_and_ttl(self):
        gone=self.home/'gone'; self.visit(gone); gone.rmdir(); recent=self.home/'recent'; self.visit(recent); recent.rmdir()
        with sqlite3.connect(self.db()) as db: db.execute('UPDATE visits SET last_seen=? WHERE path=?',(int(time.time())-91*86400,str(gone)))
        self.cli('history','list')
        with sqlite3.connect(self.db()) as db: self.assertEqual(db.execute('select count(*) from visits where path=?',(str(gone),)).fetchone()[0],0); self.assertEqual(db.execute('select count(*) from visits where path=?',(str(recent),)).fetchone()[0],1)
        self.cli('history','prune'); self.assertEqual(json.loads(self.cli('explain','--json','recent').stdout)['inventory'],1)
        self.cli('history','prune','--apply'); self.assertEqual(json.loads(self.cli('explain','--json','recent').stdout)['inventory'],0)
    @unittest.skipUnless(sys.platform.startswith('linux'),'backend preflight uses an isolated unavailable Linux Secret Service')
    def test_credential_preflight_before_prompt(self):
        self.env['DBUS_SESSION_BUS_ADDRESS']='unix:path='+str(self.root/'absent-service')
        with self.process('credential','set') as t:
            self.assertEqual(t.wait(),5); self.assertNotIn(b'Key:',t.output); self.assertNotIn(b'API key:',t.output)
    def test_status_disconnect_is_stopping_not_error(self):
        self.cli('config','set','semantic','on')
        with self.reply(close=True): self.assertEqual(self.cli('adapter','status').stdout,b'adapter: stopping\n')
    def test_stop_preserves_other_profiles(self):
        self.cli('config','set','semantic','on'); helpers=[]; envs=[]
        other=dict(self.env,J_JUMP_HOME=str(self.root/'other')); self.cli('config','set','semantic','on',env=other)
        try:
            for env in (self.env,dict(self.env,HTTPS_PROXY='http://127.0.0.1:9'),other):
                helpers.append(subprocess.Popen([str(BIN),'adapter-serve'],env=env,cwd=self.cwd,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,start_new_session=True)); envs.append(env)
                end=time.monotonic()+3
                while self.cli('adapter','status',env=env).stdout!=b'adapter: running\n':
                    self.assertLess(time.monotonic(),end); time.sleep(.02)
            self.cli('adapter','stop'); end=time.monotonic()+3
            while any(p.poll() is None for p in helpers[:2]) and time.monotonic()<end: time.sleep(.02)
            self.assertTrue(all(p.poll() is not None for p in helpers[:2])); self.assertIsNone(helpers[2].poll())
        finally:
            for p in helpers:
                if p.poll() is None: p.kill()
                p.wait(timeout=3)
if __name__=='__main__': unittest.main()
