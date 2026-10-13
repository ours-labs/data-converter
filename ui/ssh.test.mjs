import {describeError} from './errors.mjs';
import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {JSDOM} from 'jsdom';
import {parseSSH,connectionFields} from './ssh.mjs';
import {createTranslator} from './i18n.mjs';
import {canAdvance,periodValid,splitValid} from './state.mjs';
test('SSH grammar: options, Windows paths, aliases and IPv6',()=>{
 for(const text of ['pi@host','ssh pi@host','ssh -l pi host'])assert.deepEqual(parseSSH(text),{host:'host',user:'pi',port:'',identity_file:'',auth_mode:'existing'});
 assert.deepEqual(parseSSH('ssh -p 2222 pi@host -i "C:\\日本語\\a b"'),{host:'host',user:'pi',port:'2222',identity_file:'C:\\日本語\\a b',auth_mode:'key'});
 for(const host of ['alias','192.0.2.1','[2001:db8::1]','2001:db8::1','fe80::1%eth0'])assert.ok(parseSSH('ssh pi@'+host).host);
 assert.equal(connectionFields({ssh_host:'pi@host',ssh_port:2222}).user,'pi');
});
test('SSH grammar refuses ambiguous and executable input',()=>{
 for(const text of ['', 'ssh', 'ssh -p 0 host','ssh -p 65536 host','ssh -p 1.5 host','ssh -p 22 -p 23 host','ssh -l pi pi@host','ssh -l pi -l pi host','ssh -o ProxyCommand=bad host','ssh host ls','ssh host;echo bad','ssh host | cat','ssh host >file','ssh $(bad)','ssh `bad`','ssh "host','a@b@c','ssh -x host','ssh @host','ssh pi@-host','ssh pi@::::','ssh pi@[bad]','ssh host\nwhoami'])assert.throws(()=>parseSSH(text),text);
});
const html=readFileSync(new URL('./index.html',import.meta.url),'utf8');const source=readFileSync(new URL('./app.js',import.meta.url),'utf8').replace(/^import .*;\r?\n/gm,'');
async function app(invoke){const {window:w}=new JSDOM(html,{url:'http://localhost',runScripts:'outside-only',pretendToBeVisual:true});w.scrollTo=()=>{};Object.assign(w,{parseSSH,connectionFields,createTranslator,describeError,canAdvance,periodValid,splitValid});w.__TAURI__={core:{invoke}};w.eval(source+"\nwindow.setTestStep=value=>{step=value;show()};window.setTestCounts=(rows,records)=>{exportEstimate=rows===null?null:{rows,bytes:1,time:Date.now()};scanResult=records===null?null:{records};refreshButtons()};window.setTestBusy=value=>{busy=value;refreshButtons()};");await new Promise(r=>setImmediate(r));return w;}
test('command apply, edit, invalid atomicity and legacy roundtrip',async()=>{
 const w=await app(async()=>({})),el=id=>w.document.getElementById(id);
 el('ssh-command').value='ssh -p 2222 pi@host';await el('apply-ssh').onclick();assert.equal(el('ssh_user').value,'pi');assert.equal(el('ssh_host').value,'host');assert.equal(el('ssh_port').value,'2222');
 el('ssh-command').value='ssh bad extra';await el('apply-ssh').onclick();assert.equal(el('ssh_host').value,'host');assert.match(el('notice').textContent,/ODC-CONFIG-001/);
 w.eval('fill({ssh_host:"pi@legacy",ssh_port:23,auth_mode:"password"})');assert.equal(el('ssh_user').value,'pi');assert.equal(w.eval('profile().ssh_host'),'pi@legacy');
 w.eval('fill(profile())');assert.equal(w.eval('profile().ssh_host'),'pi@legacy');w.close();
});
test('history restores no secrets, no automatic connection, deletes and toggles',async()=>{
 let h={enabled:true,entries:[{ssh_host:'pi@host',auth_mode:'key',identity_file:'C:/a b',ssh_port:2222,last_success:1}]};const calls=[];
 const w=await app(async(name,args)=>{calls.push([name,args]);if(name==='connection_history'){if(args.action==='clear')h.entries=[];if(args.action==='enabled')h.enabled=args.value;return h;}return {};});const el=id=>w.document.getElementById(id);
 el('ssh_secret').value='old secret';el('dbpass').value='old db';el('history-list').value='0';await el('history-load').onclick();assert.equal(el('ssh_secret').value,'');assert.equal(el('dbpass').value,'');assert.equal(el('identity_file').value,'C:/a b');assert.equal(el('ssh_host').value,'host');assert.ok(!calls.some(c=>c[0]==='inspect'));
 el('history-search').value='missing';el('history-search').oninput();assert.equal(el('history-list').options.length,0);
 el('history-enabled').checked=false;await el('history-enabled').onchange();assert.equal(h.enabled,false);
 w.confirm=()=>false;await el('history-clear').onclick();assert.equal(h.entries.length,1);w.confirm=()=>true;await el('history-clear').onclick();assert.equal(h.entries.length,0);w.close();
});
test('SSH success needs no Python; history failures remain nonfatal',async()=>{
 const calls=[];const w=await app(async(name,args)=>{calls.push([name,args]);if(name==='connection_history')throw Error('unavailable');if(name==='inspect'){if(args.action==='connect')return {connected:true,history_warning:'failed'};throw Error('Python unavailable');}return {};});const el=id=>w.document.getElementById(id);
 el('ssh_host').value='example-host';await el('test-ssh').onclick();assert.equal(el('ssh-status').textContent,'SSH authentication verified');assert.equal(el('environment-status').textContent,'Python-free; database forwarding not yet checked');assert.match(el('history-warning').textContent,/SSH authentication succeeded/);assert.equal(el('test-db').disabled,false);
 assert.deepEqual(calls.filter(c=>c[0]==='inspect').map(c=>c[1].action),['connect']);w.close();
});

test('input changes invalidate both stages and history switches language',async()=>{
 const w=await app(async(name,args)=>name==='connection_history'?{enabled:true,entries:[{ssh_host:'pi@host',auth_mode:'existing',last_success:1}]}:{});const el=id=>w.document.getElementById(id);
 el('ssh_host').value='example-host';await el('test-ssh').onclick();assert.equal(el('test-db').disabled,false);el('ssh_user').value='other';el('ssh_user').dispatchEvent(new w.Event('input'));assert.equal(el('test-db').disabled,true);assert.equal(el('ssh-status').textContent,'Not checked');
 el('language').value='ja';el('language').onchange();assert.match(el('history-list').textContent,/SSH設定を使用/);el('language').value='en';el('language').onchange();assert.match(el('history-list').textContent,/Use SSH configuration/);w.close();
});

test('action failures are adjacent, accessible, safe and cleared on retry',async()=>{
 let fail=true;const w=await app(async(name)=>{if(name==='profiles'||name==='connection_history')return {};if(fail)throw Error('<img src=x onerror=bad> connection refused');return null;});const el=id=>w.document.getElementById(id);let scrolled;w.HTMLElement.prototype.scrollIntoView=function(){scrolled=this.id};
 el('ssh_host').value='example-host';el('auth').value='key';el('identity_file').value='key';w.eval('visibility()');
 for(const id of ['test-ssh','test-db','preview','pick-key','pick-output','save','estimate','pick-csv','pick-parent','split-execute']){
  if(['pick-csv','pick-parent','split-execute'].includes(id)){el('operation').value='local';el('operation').onchange();}w.setTestStep(['preview'].includes(id)?1:['pick-output','save','estimate'].includes(id)?3:0);await el(id).onclick();const message=el(`${id}-error`);assert.ok(message,id);assert.equal(message.previousElementSibling,el(id).closest('.actions,.inline')||el(id).closest('label')||el(id));assert.equal(message.getAttribute('role'),'alert');assert.equal(w.document.activeElement,message,`focus ${id}`);assert.equal(scrolled,message.id);assert.equal(message.querySelector('img'),null);assert.ok(el(id).getAttribute('aria-describedby').includes(message.id));
 }
 fail=false;await el('pick-output').onclick();assert.equal(el('pick-output-error'),null);assert.equal(el('pick-output').getAttribute('aria-describedby'),null);
 el('ssh_host').dispatchEvent(new w.Event('input',{bubbles:true}));assert.equal(el('test-ssh-error'),null);assert.equal(el('ssh_host').value,'example-host');w.close();
});

test('local errors translate; polling repeats do not steal focus; cancellation can retry',async()=>{
 let statusFails=true,cancelFails=true;const w=await app(async(name)=>{if(name==='status'){if(statusFails)throw Error('status unavailable');return {running:true,received_bytes:0};}if(name==='cancel'&&cancelFails)throw Error('cancel unavailable');return {};});const el=id=>w.document.getElementById(id);let scrolls=0;w.HTMLElement.prototype.scrollIntoView=()=>scrolls++;
 el('ssh-command').value='ssh bad extra';await el('apply-ssh').onclick();assert.match(el('apply-ssh-error').textContent,/Check the SSH input/);el('language').value='ja';el('language').onchange();assert.match(el('apply-ssh-error').textContent,/SSH/);
 el('output').value='test.csv';w.setTestStep(4);await el('execute').onclick();await w.eval('poll()');const first=scrolls,originalTextNode=el('run-status-error').firstChild;el('cancel').focus();await w.eval('poll()');assert.equal(el('run-status-error').firstChild,originalTextNode);assert.equal(scrolls,first);assert.equal(w.document.activeElement,el('cancel'));
 statusFails=false;await w.eval('poll()');assert.equal(el('run-status-error'),null);
 await el('cancel').onclick();assert.ok(el('cancel-error'));assert.equal(el('cancel').disabled,false);cancelFails=false;await el('cancel').onclick();assert.equal(el('cancel-error'),null);assert.equal(el('cancel').disabled,true);w.close();
});

test('coded details stay collapsed and redacted; offline help preserves the input',async()=>{
 const calls=[];const w=await app(async(name,args)=>{calls.push([name,args]);if(name==='inspect')throw 'ODC-DB-002: private-test-secret <img src=x>';if(name==='open_error_help')throw 'offline';return {};});const el=id=>w.document.getElementById(id);
 el('ssh_host').value='example-host';el('dbpass').value='private-test-secret';await el('test-db').onclick();const message=el('test-db-error');assert.match(message.querySelector('strong').textContent,/ODC-DB-002/);assert.equal(message.querySelector('details').open,false);assert.ok(!message.textContent.includes('private-test-secret'));assert.equal(message.querySelector('img'),null);
 await message.querySelector('button').onclick();assert.deepEqual(calls.find(c=>c[0]==='open_error_help')[1].code,'ODC-DB-002');assert.match(message.querySelector('button').textContent,/bundled advice/);assert.equal(el('ssh_host').value,'example-host');assert.equal(el('dbpass').value,'private-test-secret');
 el('language').value='ja';el('language').onchange();assert.match(message.querySelector('strong').textContent,/認証に失敗/);assert.match(message.querySelector('strong').textContent,/ODC-DB-002/);w.close();
});


test('CSV info shows size before counting, updates header limits, rejects stale results and cancels',async()=>{
 const pending=[];const calls=[];const w=await app(async(name,args)=>{calls.push([name,args]);if(name==='csv_metadata')return {name:args.input,bytes:169729582};if(name==='scan_csv')return new Promise((resolve,reject)=>pending.push({resolve,reject}));return {};});const el=id=>w.document.getElementById(id);
 el('operation').value='local';el('operation').onchange();el('split-input').value='first.csv';const first=el('scan-refresh').onclick();await new Promise(r=>setImmediate(r));
 assert.match(el('csv-info-values').textContent,/169.73 MB/);assert.match(el('csv-scan-status').textContent,/Counting/);assert.equal(el('split-limit').disabled,false);
 el('split-input').value='second.csv';const second=el('scan-refresh').onclick();await new Promise(r=>setImmediate(r));pending[0].resolve({records:900000,bytes:9});await first;assert.doesNotMatch(el('csv-info-values').textContent,/900,000/);
 pending[1].resolve({records:647463,bytes:169729582});await second;assert.match(el('csv-info-values').textContent,/647,462/);assert.match(el('csv-info-values').textContent,/Files after splitting7/);assert.match(el('csv-info-values').textContent,/99,999/);
 el('split-limit').value='647462';el('split-limit').dispatchEvent(new w.Event('input'));assert.match(el('csv-info-values').textContent,/Files after splitting2/);el('split-count').checked=false;el('split-count').dispatchEvent(new w.Event('input'));assert.match(el('csv-info-values').textContent,/Files after splitting1/);assert.equal(pending.length,2);
 el('language').value='ja';el('language').onchange();assert.match(el('csv-info-values').textContent,/データ件数/);assert.match(el('csv-info-values').textContent,/647,462/);
 const third=el('scan-refresh').onclick();await new Promise(r=>setImmediate(r));el('scan-cancel').onclick();pending[2].resolve({records:999,bytes:1});await third;assert.match(el('csv-scan-status').textContent,/中止/);assert.equal(el('csv-info-values').querySelectorAll('dd')[2].textContent,'—');assert.ok(calls.some(([name])=>name==='cancel_csv_scan'));
 const fourth=el('scan-refresh').onclick();await new Promise(r=>setImmediate(r));pending[3].reject(Error('invalid encoding'));await fourth;assert.ok(el('scan-refresh-error'));assert.match(el('csv-scan-status').textContent,/集計できません/);
 const fifth=el('scan-refresh').onclick();await new Promise(r=>setImmediate(r));el('split-input').value='';el('split-input').dispatchEvent(new w.Event('input'));assert.equal(el('scan-cancel').hidden,true);pending[4].resolve({records:3,bytes:1});await fifth;w.close();
});


test('PostgreSQL split estimates update without queries and invalidate on source changes',async()=>{
 let rows=647462,fail=false,pending=null,delay=false;const calls=[];
 const w=await app(async(name,args)=>{calls.push([name,args]);if(name==='inspect'&&args.action==='estimate'){if(fail)throw Error('DB unavailable');if(delay)return new Promise(resolve=>pending=resolve);return {rows,estimated_bytes:169729582};}if(name==='destination')return {free_bytes:10};if(name==='inspect'&&args.action==='list')throw Error('list unavailable');return {};});const el=id=>w.document.getElementById(id),values=()=>[...el('export-info-values').querySelectorAll('dd')].map(e=>e.textContent);
 el('ssh_host').value='example-host';el('export-split').checked=true;el('export-split').onchange();w.setTestStep(3);assert.equal(values()[0],'—');el('output').value='C:/out';await el('estimate').onclick();assert.deepEqual(values().slice(0,4),['647,462','169.73 MB','7','99,999 + 1 header record']);assert.match(el('estimate-result').textContent,/not be enough/);
 el('export-limit').value='647462';el('export-limit').dispatchEvent(new w.Event('input'));assert.equal(values()[2],'2');el('export-count').checked=false;el('export-count').dispatchEvent(new w.Event('input'));assert.equal(values()[2],'1');assert.equal(calls.filter(([n,a])=>n==='inspect'&&a.action==='estimate').length,1);
 el('output').value='C:/other';el('output').oninput();assert.equal(values()[0],'647,462');assert.doesNotMatch(el('estimate-result').textContent,/not be enough/);
 el('language').value='ja';el('language').onchange();assert.match(el('export-info-values').textContent,/対象データ件数/);assert.match(values()[3],/ヘッダー1行/);
 el('export-limit').value='0';el('export-limit').dispatchEvent(new w.Event('input'));assert.equal(values()[2],'—');el('export-limit').value='1';el('export-limit').dispatchEvent(new w.Event('input'));assert.equal(values()[2],'647,462');el('export-count').checked=true;el('export-count').dispatchEvent(new w.Event('input'));assert.equal(values()[2],'—');
 el('export-limit').value='100000';rows=0;await el('estimate').onclick();assert.equal(values()[0],'0');assert.equal(values()[2],'0');
 rows=10;await el('estimate').onclick();el('period').dispatchEvent(new w.Event('input'));assert.equal(values()[0],'—');await el('estimate').onclick();el('no-columns').onclick();assert.equal(values()[0],'—');
 await el('estimate').onclick();fail=true;await el('estimate').onclick();assert.equal(values()[0],'—');assert.ok(el('estimate-error'));fail=false;
 delay=true;const request=el('estimate').onclick();await new Promise(r=>setImmediate(r));el('start').dispatchEvent(new w.Event('input'));pending({rows:200,estimated_bytes:100});await request;assert.equal(values()[0],'—');delay=false;
 await el('estimate').onclick();await el('database_name').onchange();assert.equal(values()[0],'—');
 await el('estimate').onclick();el('ssh_host').dispatchEvent(new w.Event('input'));assert.equal(values()[0],'—');w.close();
});

test('row controls use source count, preserve arbitrary integers and stay near estimates',async()=>{
 const w=await app(async name=>name==='profiles'?{}:name==='history'?[]:{}),el=id=>w.document.getElementById(id);
 for(const [rows,expected] of [[null,1],[0,1],[99,1],[100,10],[999,10],[1000,100],[9999,100],[10000,1000],[99999,1000],[100000,10000],[647462,10000],[1000000,100000]])assert.equal(w.eval(`rowIncrement(${rows})`),expected);
 assert.ok(el('estimate').compareDocumentPosition(el('export-limit'))&w.Node.DOCUMENT_POSITION_FOLLOWING);
 assert.ok(el('export-info-values').compareDocumentPosition(el('export-limit'))&w.Node.DOCUMENT_POSITION_FOLLOWING);
 w.setTestCounts(647462,647463);
 for(const prefix of ['export','split']){
  const input=el(prefix+'-limit');input.value='100000';el(prefix+'-increase').click();assert.equal(input.value,'110000');el(prefix+'-decrease').click();assert.equal(input.value,'100000');
  input.value='123456';input.dispatchEvent(new w.KeyboardEvent('keydown',{key:'ArrowUp',bubbles:true,cancelable:true}));assert.equal(input.value,'133456');assert.equal(input.validity.stepMismatch,false);
  input.value='2';el(prefix+'-decrease').click();assert.equal(input.value,'2');
  assert.match(el(prefix+'-increment').textContent,/10,000/);
 }
 el('export-count').checked=false;el('export-count').dispatchEvent(new w.Event('input'));el('export-decrease').click();assert.equal(el('export-limit').value,'1');
 el('language').value='ja';el('language').onchange();assert.match(el('export-increment').textContent,/10,000行ずつ/);
 w.setTestBusy(true);el('export-increase').click();assert.equal(el('export-limit').value,'1');
 w.setTestBusy(false);w.setTestCounts(null,null);el('export-increase').click();assert.equal(el('export-limit').value,'2');assert.match(el('split-increment').textContent,/1行ずつ/);
 w.close();
});

test('split toggle preserves separate destinations and derives only a first-time target',async()=>{
 let choice=null;const requests=[];
 const w=await app(async(name,args)=>{requests.push([name,args]);return name==='choose'?choice:{};}),el=id=>w.document.getElementById(id);
 const toggle=value=>{el('export-split').checked=value;el('export-split').onchange()};
 el('output').value='C:\\CSV\\日本語.csv';toggle(true);assert.equal(el('output').value,'C:\\CSV');toggle(false);assert.equal(el('output').value,'C:\\CSV\\日本語.csv');
 toggle(true);el('output').value='D:\\分割先';toggle(false);assert.equal(el('output').value,'C:\\CSV\\日本語.csv');toggle(true);assert.equal(el('output').value,'D:\\分割先');
 await el('pick-output').onclick();assert.equal(el('output').value,'D:\\分割先');choice='E:/chosen';await el('pick-output').onclick();toggle(false);toggle(true);assert.equal(el('output').value,'E:/chosen');
 el('language').value='ja';el('language').onchange();assert.equal(el('output').value,'E:/chosen');
 w.eval('fill({output_path:"Z:/new",split_options:{limit:100}})');toggle(false);assert.equal(el('output').value,'Z:/new/sensors.csv');toggle(true);assert.equal(el('output').value,'Z:/new');
 for(const [file,folder]of [['C:/a.csv','C:/'],['C:\\a.csv','C:\\'],['\\\\server\\share\\a.csv','\\\\server\\share'],['a.csv','.'],['/a.csv','/']]){
  w.eval(`fill(${JSON.stringify({output_path:file})})`);toggle(true);assert.equal(el('output').value,folder);toggle(false);assert.equal(el('output').value,file);
 }
 w.eval('fill({})');toggle(true);assert.equal(el('output').value,'');assert.equal(requests.filter(([name])=>name==='begin_export').length,0);w.close();
});
