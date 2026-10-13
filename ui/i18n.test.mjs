import {describeError} from './errors.mjs';
import {parseSSH,connectionFields} from './ssh.mjs';
import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {JSDOM} from 'jsdom';
import {createTranslator,readLanguage,saveLanguage,translate,STORAGE_KEY} from './i18n.mjs';
import {canAdvance,periodValid,splitValid} from './state.mjs';
const html=readFileSync(new URL('./index.html',import.meta.url),'utf8');
const source=readFileSync(new URL('./app.js',import.meta.url),'utf8');
function page(){return new JSDOM(html,{url:'http://localhost',runScripts:'outside-only',pretendToBeVisual:true});}
test('初回は英語、未知の保存値と保存不能時も英語',()=>{
 assert.equal(readLanguage({getItem:()=>null}),'en');assert.equal(readLanguage({getItem:()=> 'fr'}),'en');assert.equal(readLanguage(undefined),'en');assert.doesNotThrow(()=>saveLanguage(undefined,'ja'));
});
test('5画面・placeholder・初期進捗を英語化し、日本語へ往復して復元',()=>{
 const {window:w}=page();const i=createTranslator(w.document,w.localStorage);
 assert.equal(w.document.documentElement.lang,'en');assert.equal(w.document.querySelector('[data-step="0"] h2').textContent,'Connection');
 assert.equal(w.document.getElementById('elapsed').textContent,'0 s');assert.equal(w.document.getElementById('ssh_host').placeholder,'e.g. server-alias');
 const nodes=[...w.document.querySelectorAll('section,h2,aside,#host-dialog')].map(e=>e.textContent).join('');assert.doesNotMatch(nodes,/[ぁ-んァ-ン一-龠]/);
 i.change('ja');assert.equal(w.document.querySelector('[data-step="0"] h2').textContent,'接続設定');assert.equal(w.document.getElementById('elapsed').textContent,'0 秒');
 i.change('en');assert.equal(w.document.querySelector('[data-step="0"] h2').textContent,'Connection');w.close();
});
test('選択は保存・復元されるが接続profileから独立',()=>{
 const {window:w}=page();const i=createTranslator(w.document,w.localStorage);i.change('ja');assert.equal(w.localStorage.getItem(STORAGE_KEY),'ja');
 assert.equal(createTranslator(page().window.document,w.localStorage).language,'ja');i.change('en');assert.equal(readLanguage(w.localStorage),'en');w.close();
});
test('動的な完了・容量不足・確認メッセージの値は翻訳しない',()=>{
 assert.equal(translate('設定「完了」を削除しますか？','en'),'Delete configuration “完了”?');
 assert.equal(translate('647,462行 / 169.73 MB / 6.73秒\n保存先：C:\\日本語\\完了.csv','en'),'647,462 rows / 169.73 MB / 6.73 s\nDestination: C:\\日本語\\完了.csv');
 assert.equal(translate('5行・CSV推定 1.00 MB（最大1,000行から推定） / 保存先の空き 0.50 MB / 空き容量不足の可能性があります。','en'),'5 rows · estimated CSV 1.00 MB (sample: up to 1,000 rows) / free space 0.50 MB / There may not be enough free space.');
});
test('アプリ実装で接続状態・設定名・プレビューを保持して切り替える',async()=>{
 const {window:w}=page();w.scrollTo=()=>{};Object.assign(w,{parseSSH,connectionFields});w.createTranslator=createTranslator;w.canAdvance=canAdvance;w.periodValid=periodValid;w.splitValid=splitValid;
 const requests=[];w.__TAURI__={core:{invoke:async(name,args)=>{requests.push([name,args]);if(name==='profiles')return {'完了':{}};if(args.action==='ssh')return {};if(args.action==='databases')return {names:['完了'],current:'完了'};if(args.action==='list')return [['public','接続設定','完了','text']];if(args.action==='preview')return {columns:['完了'],rows:[['接続設定']]};return {};}}};
 w.eval(source.replace(/^import .*;\r?\n/gm,''));await new Promise(r=>setImmediate(r));
 const el=id=>w.document.getElementById(id);assert.equal(el('saved').options[1].textContent,'完了');
 el('ssh_host').value='example-host';await el('test-ssh').onclick();await el('test-db').onclick();await el('preview').onclick();
 assert.equal(el('ssh-status').textContent,'SSH authentication verified');assert.equal(el('table').value,'接続設定');
 const preview=el('preview-table').textContent;assert.equal(preview,'完了接続設定');
 el('language').value='ja';el('language').onchange();assert.equal(el('ssh-status').textContent,'SSH認証を確認済み');assert.equal(el('preview-table').textContent,preview);assert.equal(el('next').disabled,false);
 el('language').value='en';el('language').onchange();assert.equal(el('ssh-status').textContent,'SSH authentication verified');assert.equal(el('preview-table').textContent,preview);assert.equal(el('saved').options[1].textContent,'完了');
 assert.ok(requests.length>=5);
 w.__TAURI__.core.invoke=async(name,args)=>{if(name==='inspect'&&args.action==='estimate')return {rows:5,estimated_bytes:1000000};if(name==='destination')return {exists:false,free_bytes:500000};if(name==='status')return {running:false,received_bytes:1000000,completion:{status:'complete',result:{rows:5,bytes:1000000,total_seconds:1,output:'C:\\完了.csv'}}};return {};};
 el('output').value='C:\\完了.csv';el('next').onclick();el('next').onclick();el('next').onclick();await el('estimate').onclick();
 assert.match(el('estimate-result').textContent,/There may not be enough free space/);assert.match(el('summary').textContent,/All dates/);assert.match(el('summary').textContent,/接続設定/);
 el('language').value='ja';el('language').onchange();assert.match(el('estimate-result').textContent,/空き容量不足/);assert.match(el('summary').textContent,/全期間/);
 el('language').value='en';el('language').onchange();el('next').onclick();w.setInterval=()=>1;await el('execute').onclick();assert.equal(el('language').disabled,true);
 await w.eval('poll()');assert.equal(el('run-status').textContent,'Complete');assert.equal(el('result').textContent,'5 rows / 1.00 MB / 1.00 s\nDestination: C:\\完了.csv');
 el('language').value='ja';el('language').onchange();assert.equal(el('run-status').textContent,'完了');assert.match(el('result').textContent,/保存先：C:\\完了.csv/);w.close();
});

test('ローカル分割は接続不要・header設定・中止・結果・言語切替を保持',async()=>{
 const {window:w}=page();w.scrollTo=()=>{};Object.assign(w,{parseSSH,connectionFields});Object.assign(w,{createTranslator,describeError,canAdvance,periodValid,splitValid});w.setInterval=()=>1;
 const requests=[];let completion={status:'complete',result:{rows:4,files:2,bytes:50,total_seconds:0.1,output:'C:/日本語/result'}};
 w.__TAURI__={core:{invoke:async(name,args)=>{requests.push([name,args]);if(name==='status')return {running:false,received_bytes:50,completion};return {};}}};
 w.eval(source.replace(/^import .*;\r?\n/gm,''));await new Promise(r=>setImmediate(r));const el=id=>w.document.getElementById(id);
 el('operation').value='local';el('operation').onchange();assert.equal(el('local-split').hidden,false);assert.equal(el('split-count').checked,true);
 el('split-input').value='C:/日本語/in.csv';el('split-parent').value='C:/日本語';el('split-limit').value='1';w.eval('refreshButtons()');assert.equal(el('split-execute').disabled,true);
 el('split-header').checked=false;w.eval('refreshButtons()');assert.equal(el('split-count').disabled,true);assert.equal(el('split-execute').disabled,false);
 el('split-encoding').value='cp932';el('split-delimiter').value='tab';await el('split-execute').onclick();const call=requests.find(x=>x[0]==='begin_split');assert.equal(call[1].options.delimiter,'\t');assert.equal(call[1].options.header,false);assert.equal(call[1].options.encoding,'cp932');assert.equal(el('operation').disabled,true);
 await el('cancel').onclick();assert.ok(requests.some(x=>x[0]==='cancel'));await w.eval('poll()');assert.match(el('result').textContent,/4 records \/ 2 files/);assert.equal(el('open-output').hidden,false);
 el('language').value='ja';el('language').onchange();assert.match(el('result').textContent,/4件 \/ 2ファイル/);
 el('split-again').onclick();assert.equal(el('local-split').hidden,false);
 completion={status:'complete',result:{rows:0,files:0,bytes:0,total_seconds:0.1,output:null,empty:true}};await el('split-execute').onclick();await w.eval('poll()');assert.equal(el('open-output').hidden,true);assert.match(el('notice').textContent,/データがない/);w.close();
});
test('保存前分割はfolder選択と上書きなしの設定を渡す',async()=>{
 const {window:w}=page();w.scrollTo=()=>{};Object.assign(w,{parseSSH,connectionFields});Object.assign(w,{createTranslator,describeError,canAdvance,periodValid,splitValid});w.setInterval=()=>1;
 const requests=[];w.__TAURI__={core:{invoke:async(name,args)=>{requests.push([name,args]);return name==='choose'?'C:/output':{};}}};
 w.eval(source.replace(/^import .*;\r?\n/gm,''));await new Promise(r=>setImmediate(r));const el=id=>w.document.getElementById(id);
 el('export-split').checked=true;el('export-split').onchange();await el('pick-output').onclick();assert.equal(requests.find(x=>x[0]==='choose')[1].kind,'folder');
 el('export-limit').value='10';el('export-count').checked=false;await el('execute').onclick();const call=requests.find(x=>x[0]==='begin_export');assert.equal(call[1].splitOptions.limit,10);assert.equal(call[1].splitOptions.count_header,false);assert.equal(call[1].overwrite,false);assert.ok(!requests.some(x=>x[0]==='destination'));w.close();
});

test('指定行数は安全な整数とheader計数に応じた最小値を要求',()=>{
 for(const value of ['',0,-1,1.5,'abc',Infinity,Number.MAX_SAFE_INTEGER+1])assert.equal(splitValid(value,false,false),false);
 assert.equal(splitValid(1,false,true),true);assert.equal(splitValid(1,true,false),true);assert.equal(splitValid(1,true,true),false);assert.equal(splitValid(2,true,true),true);
});

test('保存済み分割設定を復元し無効化したらprofileから外す',async()=>{
 const {window:w}=page();w.scrollTo=()=>{};Object.assign(w,{parseSSH,connectionFields});Object.assign(w,{createTranslator,describeError,canAdvance,periodValid,splitValid});w.__TAURI__={core:{invoke:async()=>({})}};
 w.eval(source.replace(/^import .*;\r?\n/gm,''));await new Promise(r=>setImmediate(r));
 w.eval('fill({output_path:"C:/folder",split_options:{limit:37,header:true,count_header:false,delimiter:",",encoding:"utf8"}})');
 const el=id=>w.document.getElementById(id);assert.equal(el('export-split').checked,true);assert.equal(el('export-limit').value,'37');assert.equal(el('export-count').checked,false);assert.equal(w.eval('profile().split_options.limit'),37);
 el('export-split').checked=false;assert.equal(w.eval('profile().split_options'),undefined);w.close();
});

test('共有言語の初回移行で旧GUI設定を保持し、以後は共有値を復元',async()=>{
 for(const saved of [null,'en']) {
  const {window:w}=page();w.scrollTo=()=>{};w.localStorage.setItem(STORAGE_KEY,'ja');
  Object.assign(w,{parseSSH,connectionFields,createTranslator,describeError,canAdvance,periodValid,splitValid});
  const calls=[];w.__TAURI__={core:{invoke:async(name,args)=>{calls.push([name,args]);return name==='language_preference'?saved:{};}}};
  w.eval(source.replace(/^import .*;\r?\n/gm,''));await new Promise(r=>setImmediate(r));
  assert.equal(w.document.getElementById('language').value,saved??'ja');
  assert.equal(calls.some(([name,args])=>name==='language_preference'&&args.value==='ja'),saved===null);w.close();
 }
});
