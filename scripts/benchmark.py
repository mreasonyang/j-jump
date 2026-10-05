#!/usr/bin/env python3
"""Synthetic end-to-end process timings; does not prove human utility or real recall.

Covers direct path, unique match, broad all-match and genuine no-match queries at
100/1000/10000 entries so AC-003's broad-query coverage is reproducible from this
tracked script. Warm cache only; every sample is a fresh process.
"""
import argparse,hashlib,json,os,pathlib,platform,statistics,subprocess,tempfile,time,sqlite3
p=argparse.ArgumentParser();p.add_argument('--binary',type=pathlib.Path,default=pathlib.Path('target/release/jjump'));p.add_argument('--samples',type=int,default=1000);p.add_argument('--out',type=pathlib.Path,required=True);a=p.parse_args();binary=a.binary.resolve();results=[]
with tempfile.TemporaryDirectory() as temp:
 root=pathlib.Path(temp).resolve();home=root/'home';home.mkdir();neutral=home/'neutral';neutral.mkdir();env=dict(os.environ,HOME=str(home),J_JUMP_HOME=str(root/'state'),PWD=str(neutral));env.pop('TYPESAFE_API_KEY',None)
 subprocess.run([binary,'doctor'],env=env,cwd=neutral,stdout=subprocess.DEVNULL,check=True)
 db=sqlite3.connect(root/'state/data/visits.db')
 for size in (100,1000,10000):
  db.execute('delete from visits');now=int(time.time())
  for i in range(size):
   d=home/('project-%05d'%i);d.mkdir(exist_ok=True);db.execute('insert into visits(path,key,count,last_seen,weight) values(?,?,?,?,?)',(str(d),str(d).lower(),i+1,now,float(i+1)))
  db.commit()
  # Expected exit code and exact stdout per mode; the broad winner is the highest-count prefix match.
  modes=[('direct',['query','--',str(home/'project-00000')],0,str(home/'project-00000')+'\n'),('local',['query','project-00000'],0,str(home/'project-00000')+'\n'),('broad',['query','project'],0,str(home/('project-%05d'%(size-1)))+'\n'),('none',['query','zzz-no-such-token'],3,'')]
  for mode,args,code,want in modes:
   check=subprocess.run([binary,*args],env=env,cwd=neutral,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
   if check.returncode!=code or check.stdout.decode()!=want:raise SystemExit(f'benchmark {mode} at {size} output mismatch: rc={check.returncode} stdout={check.stdout!r} want={want!r}')
   samples=[];cpu=[];rss=[]
   for _ in range(a.samples):
    t=time.perf_counter_ns();process=subprocess.Popen([binary,*args],env=env,cwd=neutral,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL);_,status,usage=os.wait4(process.pid,0);process.returncode=os.waitstatus_to_exitcode(status);samples.append((time.perf_counter_ns()-t)/1e6);cpu.append((usage.ru_utime+usage.ru_stime)*1000);rss.append(usage.ru_maxrss/(1024 if platform.system()=='Darwin' else 1))
    if process.returncode!=code:raise SystemExit(f'benchmark {mode} at {size} unexpected exit {process.returncode}')
   results.append({'inventory':size,'mode':mode,'n':len(samples),'expected_exit_code':code,'validated_output':True,'p50_ms':statistics.median(samples),'p95_ms':sorted(samples)[int(.95*(len(samples)-1))],'max_ms':max(samples),'first_process_ms':samples[0],'cpu_p95_ms':sorted(cpu)[int(.95*(len(cpu)-1))],'max_rss_kib':max(rss),'cache':'OS cache warm after first invocation; fresh process every sample'})
report={'schema_version':2,'host':{'os':platform.system(),'release':platform.release(),'arch':platform.machine()},'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'synthetic':True,'results':results,'limitations':['No real-user utility or recall evidence','No OS page-cache purge; first invocation is not a certified cold-cache sample','Broad/no-match modes are included so AC-003 coverage is reproducible from this tracked script','Container timings are not host certification']};a.out.parent.mkdir(parents=True,exist_ok=True);a.out.write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(results,indent=2))
