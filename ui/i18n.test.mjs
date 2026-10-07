import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {JSDOM} from 'jsdom';
import {createTranslator,readLanguage,saveLanguage,translate,STORAGE_KEY} from './i18n.mjs';
import {canAdvance,periodValid} from './state.mjs';
const html=readFileSync(new URL('./index.html',import.meta.url),'utf8');
const source=readFileSync(new URL('./app.js',import.meta.url),'utf8');
function page(){return new JSDOM(html,{url:'http://localhost',runScripts:'outside-only',pretendToBeVisual:true});}
test('初回は英語、未知の保存値と保存不能時も英語',()=>{
 assert.equal(readLanguage({getItem:()=>null}),'en');assert.equal(readLanguage({getItem:()=> 'fr'}),'en');assert.equal(readLanguage(undefined),'en');assert.doesNotThrow(()=>saveLanguage(undefined,'ja'));
});
test('5画面・placeholder・初期進捗を英語化し、日本語へ往復して復元',()=>{
 const {window:w}=page();const i=createTranslator(w.document,w.localStorage);
 assert.equal(w.document.documentElement.lang,'en');assert.equal(w.document.querySelector('h2').textContent,'Connection');
 assert.equal(w.document.getElementById('elapsed').textContent,'0 s');assert.equal(w.document.getElementById('ssh_host').placeholder,'e.g. server-alias');
 const nodes=[...w.document.querySelectorAll('section,h2,aside,#host-dialog')].map(e=>e.textContent).join('');assert.doesNotMatch(nodes,/[ぁ-んァ-ン一-龠]/);
 i.change('ja');assert.equal(w.document.querySelector('h2').textContent,'接続設定');assert.equal(w.document.getElementById('elapsed').textContent,'0 秒');
 i.change('en');assert.equal(w.document.querySelector('h2').textContent,'Connection');w.close();
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
 const {window:w}=page();w.scrollTo=()=>{};w.createTranslator=createTranslator;w.canAdvance=canAdvance;w.periodValid=periodValid;
 const requests=[];w.__TAURI__={core:{invoke:async(name,args)=>{requests.push([name,args]);if(name==='profiles')return {'完了':{}};if(args.action==='ssh')return {};if(args.action==='databases')return {names:['完了'],current:'完了'};if(args.action==='list')return [['public','接続設定','完了','text']];if(args.action==='preview')return {columns:['完了'],rows:[['接続設定']]};return {};}}};
 w.eval(source.replace(/^import .*;\r?\n/gm,''));await new Promise(r=>setImmediate(r));
 const el=id=>w.document.getElementById(id);assert.equal(el('saved').options[1].textContent,'完了');
 await el('test-ssh').onclick();await el('test-db').onclick();await el('preview').onclick();
 assert.equal(el('ssh-status').textContent,'Connection & export environment verified');assert.equal(el('table').value,'接続設定');
 const preview=el('preview-table').textContent;assert.equal(preview,'完了接続設定');
 el('language').value='ja';el('language').onchange();assert.equal(el('ssh-status').textContent,'接続・出力環境を確認済み');assert.equal(el('preview-table').textContent,preview);assert.equal(el('next').disabled,false);
 el('language').value='en';el('language').onchange();assert.equal(el('ssh-status').textContent,'Connection & export environment verified');assert.equal(el('preview-table').textContent,preview);assert.equal(el('saved').options[1].textContent,'完了');
 assert.equal(requests.length,5);
 w.__TAURI__.core.invoke=async(name,args)=>{if(name==='inspect'&&args.action==='estimate')return {rows:5,estimated_bytes:1000000};if(name==='destination')return {exists:false,free_bytes:500000};if(name==='status')return {running:false,received_bytes:1000000,completion:{status:'complete',result:{rows:5,bytes:1000000,total_seconds:1,output:'C:\\完了.csv'}}};return {};};
 el('output').value='C:\\完了.csv';el('next').onclick();el('next').onclick();el('next').onclick();await el('estimate').onclick();
 assert.match(el('estimate-result').textContent,/There may not be enough free space/);assert.match(el('summary').textContent,/All dates/);assert.match(el('summary').textContent,/接続設定/);
 el('language').value='ja';el('language').onchange();assert.match(el('estimate-result').textContent,/空き容量不足/);assert.match(el('summary').textContent,/全期間/);
 el('language').value='en';el('language').onchange();el('next').onclick();w.setInterval=()=>1;await el('execute').onclick();assert.equal(el('language').disabled,true);
 await w.eval('poll()');assert.equal(el('run-status').textContent,'Complete');assert.equal(el('result').textContent,'5 rows / 1.00 MB / 1.00 s\nDestination: C:\\完了.csv');
 el('language').value='ja';el('language').onchange();assert.equal(el('run-status').textContent,'完了');assert.match(el('result').textContent,/保存先：C:\\完了.csv/);w.close();
});
