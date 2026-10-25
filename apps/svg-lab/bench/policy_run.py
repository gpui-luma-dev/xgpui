#!/usr/bin/env python3
"""Compare bounded outputs and replay the same queue as the interactive lab."""
import argparse
import datetime
import hashlib
import json
from pathlib import Path
import platform
import subprocess

p=argparse.ArgumentParser()
p.add_argument('--binary',type=Path,default=Path('target/release/svg-lab'))
p.add_argument('--corpus',type=Path,default=Path('/tmp/xgpui-svg-corpus'))
p.add_argument('--output',type=Path,required=True)
a=p.parse_args()
a.output.mkdir(parents=True,exist_ok=True)
manifest=json.loads(Path(__file__).with_name('corpus.json').read_text())
rows=[]
metadata={'started_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'platform':platform.platform(),'binary_sha256':hashlib.sha256(a.binary.read_bytes()).hexdigest(),'budgets_mib':[16,32],'density':2,'raster_repeats':5,'sequence_ms':[[0,1],[20,2],[40,4],[60,8],[80,16],[100,4],[120,16]],'scope':'Serial isolated CPU workers; GPU/UI residency excluded. Sequence replays shared queue on a worker thread; it does not measure UI frame latency.'}
for item in manifest:
    path=a.corpus/(item['id']+'.svg')
    assert hashlib.sha256(path.read_bytes()).hexdigest()==item['sha256']
    for budget in [16,32]:
        for mode,zoom in [('raster',1),('raster',4),('raster',16),('sequence',16)]:
            cmd=[str(a.binary.resolve()),'--benchmark',str(path),'--budget-mib',str(budget),'--zoom',str(zoom),'--density','2','--iterations','5']
            if mode=='sequence':cmd+=['--sequence']
            row={'id':item['id'],'mode':mode,'zoom':zoom,'budget_mib':budget,'sha256':item['sha256']}
            try:
                result=subprocess.run(cmd,capture_output=True,text=True,timeout=90)
                row['exit_code']=result.returncode
                row['stderr']=result.stderr
                if result.returncode:row['error']=result.stdout+result.stderr
                else:row['measurement']=json.loads(result.stdout)
            except subprocess.TimeoutExpired:row['error']='90 second timeout; worker killed'
            rows.append(row)
            (a.output/'policy-raw.json').write_text(json.dumps({'metadata':metadata,'cases':rows},indent=2)+'\n')
            m=row.get('measurement',{})
            latency=m.get('final_detail_after_last_request_ms') if mode=='sequence' else m.get('raster',{}).get('median_ms')
            print(f"{item['id']:24} {budget:2} MiB {mode:8} {zoom:2}x {latency} ms",flush=True)
print(f'{len(rows)} cases saved',flush=True)
