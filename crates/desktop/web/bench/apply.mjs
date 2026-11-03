// Frontend-only validation + block application; no DOM, IPC or rendering is included.
// npm test builds .test-dist first. Usage: node bench/apply.mjs before-dir after-dir
import {readFileSync} from 'node:fs';
import {performance} from 'node:perf_hooks';
import {ChangeSet} from '@codemirror/state';
import {RustSync} from '../.test-dist/src/rust-sync.js';
for (const [label, dir] of [['full', process.argv[2]], ['incremental', process.argv[3]]]) {
  if (!dir) throw new Error('pass full-reply and incremental-reply directories from sync_bench');
  for (const blocks of [100, 1000, 5000]) {
    const fixture = JSON.parse(readFileSync(`${dir}/${blocks}.json`, 'utf8'));
    const wire = JSON.stringify({...fixture.reply, request_id: 1});
    const at = fixture.text.indexOf(`# 第${Math.floor(blocks/4/5)*5}章`) + 2;
    const samples = [];
    for (let sample = 0; sample < 44; sample++) {
      const sync = new RustSync(()=>{}, ()=>{}, ()=>{}, ()=>{}, {schedule:()=>()=>{}});
      sync.adopt({...fixture.initial, t:'open', protocol:2, text:fixture.text});
      sync.local(ChangeSet.of({from:at, to:at, insert:'字'}, fixture.text.length));
      const start = performance.now();
      sync.receiveJson(wire);
      const ns = Math.round((performance.now()-start)*1e6);
      if (sync.state !== 'ready') throw new Error(JSON.stringify(sync.failureInfo ?? sync.state));
      if (sample >= 3) {
        samples.push(ns);
        console.log(JSON.stringify({kind:'sample',label,blocks,sample:sample-3,ns}));
      }
      sync.close();
    }
    samples.sort((a,b)=>a-b);
    console.log(JSON.stringify({kind:'summary',label,blocks,p50_ns:samples[20],p95_ns:samples[38]}));
  }
}
