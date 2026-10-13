import {describeError} from './errors.mjs';
import {parseSSH,connectionFields} from './ssh.mjs';
import {createTranslator} from './i18n.mjs';
import {canAdvance,periodValid,splitValid} from './state.mjs';
const $=id=>document.getElementById(id);
let languageStorage;try{languageStorage=window.localStorage}catch{/* English remains the default. */}
const i18n=createTranslator(document,languageStorage);
const {setText}=i18n;
$('language').value=i18n.language;
let languageChanged=false;
$('language').onchange=()=>{languageChanged=true;invoke('language_preference',{value:$('language').value}).catch(()=>{});i18n.change($('language').value);for(const e of document.querySelectorAll('.action-error'))renderError(e,e.id.slice(0,-6),e.dataset.errorText);visibility();show();};
const invoke=(name,args={})=>window.__TAURI__.core.invoke(name,args);
let localMode=false,historyRows=[];
let exportEstimate=null,estimateGeneration=0;
let destinationMode=false,destinations={file:'',folder:''};
let scanId=0,scanTimer=null,scanActive=false,scanResult=null,scanMeta=null,scanState='idle';
let step=0,sshOK=false,dbOK=false,busy=false,running=false,catalog=[],hostExpected='',lastProfile={},started=0,timer=null,cancelRequested=false,displayedStep=-1;
const notify=(text,error=false)=>{setText($('notice'),text);$('notice').classList.toggle('error',error)};
const format=n=>`${(n/1e6).toFixed(2)} MB`;
const selectedColumns=()=>[...document.querySelectorAll('#columns input:checked')].map(e=>e.value);
function profile(){
 const p={ssh_host:$('ssh_user').value?`${$('ssh_user').value}@${$('ssh_host').value}`:$('ssh_host').value,database_env:$('database_env').value,timezone:'Asia/Tokyo',timeout_ms:300000};
 p.auth_mode=$('auth').value;
 if($('ssh_port').value)p.ssh_port=Number($('ssh_port').value);
 if($('auth').value==='key')p.identity_file=$('identity_file').value;
 if($('auth').value!=='existing')p._ssh_secret=$('ssh_secret').value;
 if(hostExpected)p._host_expected=hostExpected;
 if($('db-mode').value==='env')p.env_file=$('env_file').value;
 else p._database={host:$('dbhost').value,port:Number($('dbport').value),dbname:$('dbname').value,user:$('dbuser').value,password:$('dbpass').value};
 if($('database_name').value)p.database_name=$('database_name').value;
 p.schema=$('schema').value||lastProfile.schema||'public';p.table=$('table').value||lastProfile.table;
 const columns=selectedColumns();if(columns.length)p.columns=columns;
 if($('period').value==='range'){
  p.time_column=$('time_column').value;
  const type=catalog.find(r=>r[0]===p.schema&&r[1]===p.table&&r[2]===p.time_column)?.[3];
  for(const key of ['start','end'])if($(key).value)p[key]=$(key).value+(type==='timestamp with time zone'?'+09:00':'');
 }
 if($('export-split').checked)p.split_options={limit:Number($('export-limit').value),header:true,count_header:$('export-count').checked,delimiter:',',encoding:'utf8'};
 return p;
}
function options(id,values,preferred){const sel=$(id);sel.replaceChildren();for(const value of values){const o=document.createElement('option');o.value=value;o.textContent=value;sel.append(o)}if(values.includes(preferred))sel.value=preferred}
function show(){$('local-split').hidden=!localMode||step===4;$('steps').hidden=localMode;document.querySelector('footer').hidden=localMode;$('split-again').hidden=!localMode||running;$('execute').hidden=localMode;if(displayedStep!==step){displayedStep=step;requestAnimationFrame(()=>window.scrollTo(0,0))}document.querySelectorAll('[data-step]').forEach(e=>e.hidden=Number(e.dataset.step)!==step||(localMode&&step!==4));document.querySelectorAll('#steps li').forEach((e,i)=>{e.classList.toggle('active',i===step);e.classList.toggle('done',i<step)});if(localMode)setText($('step-label'),'保存済みCSVを分割');else $('step-label').textContent=`STEP ${step+1} / 5`;$('back').disabled=step===0||busy||running;$('next').hidden=step===4;refreshButtons();if(step===3)summary()}
function refreshButtons(){document.querySelectorAll('input,select,button').forEach(e=>e.disabled=e.hasAttribute('data-error-help')?false:busy||running);$('host-accept').disabled=false;$('host-reject').disabled=false;$('cancel').disabled=!running||cancelRequested;$('back').disabled=step===0||busy||running;const state={ssh:sshOK,db:dbOK,table:$('table').value,columns:selectedColumns().length,periodValid:$('period').value==='all'||Boolean($('time_column').value&&($('start').value||$('end').value)&&periodValid($('start').value,$('end').value)),output:$('output').value&&(!$('export-split').checked||splitValid($('export-limit').value,true,$('export-count').checked))};$('next').disabled=busy||running||!canAdvance(step,state);$('test-db').disabled=busy||!sshOK;$('test-ssh').disabled=busy||running;$('execute').disabled=busy||running||!state.output;syncSplitControls()}
function clearActionError(id){const control=$(id),error=$(`${id}-error`);if(!error)return;error.remove();const refs=(control.getAttribute('aria-describedby')||'').split(' ').filter(ref=>ref&&ref!==`${id}-error`);if(refs.length)control.setAttribute('aria-describedby',refs.join(' '));else control.removeAttribute('aria-describedby');}
function renderError(message,id,error){
 const info=describeError(error,id,i18n.language,[$('ssh_secret').value,$('dbpass').value]);message.replaceChildren();message.dataset.errorText=info.detail;
 const title=document.createElement('strong');title.textContent=`${info.summary} (${info.code})`;const action=document.createElement('p');action.textContent=info.action;
 const more=document.createElement('details'),label=document.createElement('summary'),detail=document.createElement('pre');setText(label,'詳細を表示');setText(detail,info.detail);more.append(label,detail);
 const help=document.createElement('button');setText(help,'GitHubの対処方法');help.dataset.errorHelp='';help.onclick=async()=>{try{await invoke('open_error_help',{code:info.code})}catch{setText(help,'ブラウザーを開けません。同梱の対処方法を確認してください。')}};
 message.append(title,action,more,help);notify(`${info.summary} (${info.code})`,true);
}
function actionError(id,error){
 const control=$(id);let message=$(`${id}-error`);const info=describeError(error,id,i18n.language,[$('ssh_secret').value,$('dbpass').value]);const changed=!message||message.dataset.errorText!==info.detail;
 if(!message){message=document.createElement('div');message.id=`${id}-error`;message.className='action-error';message.setAttribute('role','alert');message.tabIndex=-1;(control.closest('.actions,.inline')||control.closest('label')||control).after(message);control.setAttribute('aria-describedby',[control.getAttribute('aria-describedby'),message.id].filter(Boolean).join(' '));}
 if(changed){renderError(message,id,error);message.focus({preventScroll:true});message.scrollIntoView?.({block:'nearest'});}
}
async function taskFor(id,fn){if(busy||running)return;clearActionError(id);busy=true;refreshButtons();try{await fn()}catch(e){actionError(id,e)}finally{busy=false;show()}}
for(const event of ['input','change'])document.addEventListener(event,e=>{if(e.target.id==='language')return;const card=e.target.closest('.card');if(card)for(const message of card.querySelectorAll('.action-error'))clearActionError(message.id.slice(0,-6));});
async function inspect(action){validateConnection();let result=await invoke('inspect',{profile:profile(),action});if(result.host_confirmation){$('host-prompt').textContent=result.host_confirmation;$('host-dialog').showModal();const accepted=await new Promise(resolve=>{ $('host-accept').onclick=()=>{ $('host-dialog').close();resolve(true)};$('host-reject').onclick=()=>{ $('host-dialog').close();resolve(false)};$('host-dialog').oncancel=()=>resolve(false)});if(!accepted)throw 'ホスト鍵の確認を中止しました。';hostExpected=result.host_confirmation;result=await invoke('inspect',{profile:profile(),action});if(result.host_confirmation)throw 'ホスト鍵が一致しません。接続を中止しました。'}return result}
function invalidate(){clearEstimate();sshOK=false;dbOK=false;hostExpected='';catalog=[];for(const id of ['database_name','schema','table'])$(id).replaceChildren();$('columns').replaceChildren();$('preview-table').replaceChildren();setText($('ssh-status'),'未確認');setText($('environment-status'),'未確認');setText($('db-status'),'未確認');$('ssh-status').classList.remove('ok');$('db-status').classList.remove('ok');refreshButtons()}
function tableChanged(){const p=profile();const rows=catalog.filter(r=>r[0]===p.schema&&r[1]===p.table);$('columns').replaceChildren();for(const row of rows){const label=document.createElement('label');const box=document.createElement('input');box.type='checkbox';box.value=row[2];box.checked=!lastProfile.columns||lastProfile.columns.includes(row[2]);box.onchange=()=>{clearEstimate();refreshButtons()};const text=document.createElement('span');text.textContent=row[2];const small=document.createElement('small');small.textContent=row[3];label.append(box,text,small);$('columns').append(label)}options('time_column',rows.filter(r=>r[3].includes('timestamp')||r[3]==='date').map(r=>r[2]),lastProfile.time_column);$('preview-table').replaceChildren();clearEstimate();show()}
function schemaChanged(){options('table',[...new Set(catalog.filter(r=>r[0]===$('schema').value).map(r=>r[1]))],lastProfile.table);tableChanged()}
async function listTables(){clearEstimate();catalog=[];for(const id of ['schema','table','time_column'])$(id).replaceChildren();$('columns').replaceChildren();$('preview-table').replaceChildren();catalog=await inspect('list');options('schema',[...new Set(catalog.map(r=>r[0]))],lastProfile.schema);schemaChanged()}
function clearEstimate(){estimateGeneration++;exportEstimate=null;setText($('estimate-result'),'条件変更後は、必要に応じて再度見積もってください。');renderExportInfo()}
function summary(){const p=profile();$('summary').replaceChildren();for(const [label,value]of [['対象',`${p.database_name||i18n.t('接続先DB')} / ${p.schema} / ${p.table}`],['列',(p.columns||[]).join(', ')],['期間',$('period').value==='all'?'全期間':`${p.start||i18n.t('指定なし')} 以上 ～ ${p.end||i18n.t('指定なし')} 未満`],['形式','UTF-8 BOM・ヘッダーあり・日本時間'],['保存先',$('output').value]]){const dt=document.createElement('dt'),dd=document.createElement('dd');setText(dt,label);if(label==='期間'||label==='形式')setText(dd,value);else dd.textContent=value;$('summary').append(dt,dd)}}
function fill(p){lastProfile=p;invalidate();for(const id of ['env_file','database_env'])if(p[id]!=null)$(id).value=p[id];applyConnection(p.ssh_host?connectionFields(p):{host:'',user:'',port:'',identity_file:'',auth_mode:'existing'});$('auth').value=p.auth_mode||(p.identity_file?'key':p._ssh_secret?'password':'existing');$('ssh_secret').value=p._ssh_secret||'';$('db-mode').value=p._database?'direct':'env';if(p._database){for(const [id,key]of [['dbhost','host'],['dbport','port'],['dbname','dbname'],['dbuser','user'],['dbpass','password']])$(id).value=p._database[key]||''}$('output').value=p.output_path||'';$('export-split').checked=Boolean(p.split_options);destinationMode=$('export-split').checked;destinations={file:'',folder:''};destinations[destinationMode?'folder':'file']=$('output').value;$('export-limit').value=p.split_options?.limit||100000;$('export-count').checked=p.split_options?.count_header!==false;$('period').value=p.start||p.end?'range':'all';for(const key of ['start','end'])$(key).value=(p[key]||'').slice(0,16);$('save-secrets').checked=Boolean(p.saved_secrets);visibility();step=0;show()}
function visibility(){connectionSummary();renderHistory();$('key-field').hidden=$('auth').value!=='key';$('secret-field').hidden=$('auth').value==='existing';setText($('secret-label'),$('auth').value==='key'?'秘密鍵のパスフレーズ（任意）':'SSHパスワード');$('env-fields').hidden=$('db-mode').value!=='env';$('direct-fields').hidden=$('db-mode').value!=='direct';$('period-fields').hidden=$('period').value!=='range'}
async function saved(){const data=await invoke('profiles');options('saved',['',...Object.keys(data)]);setText($('saved').options[0],'新しい設定')}
$('test-ssh').onclick=()=>taskFor('test-ssh',async()=>{
 invalidate();const connected=await inspect('connect');setText($('ssh-status'),'SSH認証を確認済み');$('ssh-status').classList.add('ok');
 if(connected.history_warning)setText($('history-warning'),'履歴を保存できませんでした。接続確認は成功しています。');else await loadHistory();
 sshOK=true;setText($('environment-status'),'Python不要・DB転送は次に確認');notify('SSH接続を確認しました。次にDB接続を確認してください。');
});
$('test-db').onclick=()=>taskFor('test-db',async()=>{const names=await inspect('databases');options('database_name',names.names,lastProfile.database_name||names.current);await listTables();dbOK=true;setText($('db-status'),'接続済み');$('db-status').classList.add('ok');notify('DB接続を確認しました。出力対象を選択してください。')});
$('database_name').onchange=()=>taskFor('database_name',listTables);$('schema').onchange=schemaChanged;$('table').onchange=tableChanged;$('refresh').onclick=()=>taskFor('refresh',listTables);
$('preview').onclick=()=>taskFor('preview',async()=>{const result=await inspect('preview');const table=document.createElement('table');const head=document.createElement('tr');for(const col of result.columns){const th=document.createElement('th');th.textContent=col;head.append(th)}table.append(head);for(const row of result.rows){const tr=document.createElement('tr');for(const value of row){const td=document.createElement('td');td.textContent=value===null?'NULL':typeof value==='object'?JSON.stringify(value):String(value);tr.append(td)}table.append(tr)}$('preview-table').replaceChildren(table);notify(`${result.rows.length}行を表示しました。プレビューは各値1024文字まで。本出力には制限を適用しません。`)});
for(const id of ['all-columns','no-columns'])$(id).onclick=()=>{document.querySelectorAll('#columns input').forEach(e=>e.checked=id==='all-columns');clearEstimate();refreshButtons()};
for(const id of ['ssh_host','ssh_user','ssh_port','auth','identity_file','ssh_secret','env_file','database_env','db-mode','dbhost','dbport','dbname','dbuser','dbpass'])$(id).addEventListener('input',()=>{invalidate();visibility()});
for(const id of ['period','start','end','time_column'])$(id).addEventListener('input',()=>{visibility();clearEstimate();refreshButtons()});$('output').oninput=()=>{clearDestinationEstimate();refreshButtons();summary()};
$('back').onclick=()=>{step--;show()};$('next').onclick=()=>{step++;show()};
$('pick-key').onclick=()=>taskFor('pick-key',async()=>{const path=await invoke('choose',{kind:'key'});if(path){$('identity_file').value=path;invalidate()}});
$('pick-output').onclick=()=>taskFor('pick-output',async()=>{const path=await invoke('choose',{kind:$('export-split').checked?'folder':'output'});if(path){$('output').value=path;clearDestinationEstimate()}});
$('import').onclick=()=>taskFor('import',async()=>{const path=await invoke('choose',{kind:'profile'});if(path){fill(await invoke('import_profile',{path}));notify('設定を読み込みました。接続を確認してください。')}});
$('load').onclick=()=>taskFor('load',async()=>{if(!$('saved').value)return;const name=$('saved').value;fill(await invoke('load_profile',{name}));$('profile-name').value=name;notify('登録済み設定を読み込みました。接続を確認してください。')});
$('remove').onclick=()=>taskFor('remove',async()=>{const name=$('saved').value;if(name&&confirm(i18n.t(`設定「${name}」を削除しますか？`))){await invoke('delete_profile',{name});await saved();notify('設定を削除しました。')}});
$('save').onclick=()=>taskFor('save',async()=>{validateConnection();const p=profile();p.output_path=$('output').value;await invoke('save_profile',{name:$('profile-name').value,profile:p,saveSecrets:$('save-secrets').checked});await saved();notify('設定を登録しました。')});
$('estimate').onclick=()=>taskFor('estimate',async()=>{
 clearEstimate();const generation=estimateGeneration;
 const p=await inspect('estimate');if(generation!==estimateGeneration)return;
 exportEstimate={rows:p.rows,bytes:p.estimated_bytes,time:Date.now(),free:null};renderExportInfo();renderEstimateText();
 const output=$('output').value;if(output){const d=await invoke('destination',{output});if(generation!==estimateGeneration||output!==$('output').value)return;exportEstimate.free=d.free_bytes;renderEstimateText()}
});
function clearDestinationEstimate(){if(exportEstimate){exportEstimate.free=null;renderEstimateText()}}
function renderEstimateText(){
 if(!exportEstimate)return;const p=exportEstimate;let text=`${p.rows.toLocaleString()}行・CSV推定 ${format(p.bytes)}（最大1,000行から推定）`;
 if(p.free!==null){text+=` / 保存先の空き ${format(p.free)}`;if(p.free<p.bytes)text+=' / 空き容量不足の可能性があります。'}setText($('estimate-result'),text);
}
function renderExportInfo(){
 updateStepper('export');
 const ja=i18n.language==='ja',p=exportEstimate,split=$('export-split').checked,valid=splitValid($('export-limit').value,true,$('export-count').checked);
 const capacity=Number($('export-limit').value)-Number($('export-count').checked);
 const labels=ja?['対象データ件数（ヘッダー除く）','CSV全体の推定サイズ（分割前）','分割後のファイル数','1ファイルのデータ上限','見積取得時刻']:['Data records (excluding header)','Estimated CSV size (before splitting)','Files after splitting','Data records per file','Estimate obtained'];
 const values=[p?p.rows.toLocaleString():'—',p?format(p.bytes):'—',p&&split&&valid?Math.ceil(p.rows/capacity).toLocaleString():'—',split&&valid?capacity.toLocaleString()+(ja?' ＋ ヘッダー1行':' + 1 header record'):'—',p?new Date(p.time).toLocaleString(ja?'ja-JP':'en-US'):'—'];
 $('export-info-values').replaceChildren();labels.forEach((label,i)=>{const dt=document.createElement('dt'),dd=document.createElement('dd');dt.textContent=label;dd.textContent=values[i];$('export-info-values').append(dt,dd)});
}

$('execute').onclick=()=>taskFor('execute',async()=>{const output=$('output').value;if(!output)throw '保存先を指定してください。';const doSplit=$('export-split').checked;if(doSplit&&!splitValid($('export-limit').value,true,$('export-count').checked))throw '指定行数を確認してください。';const dest=doSplit?{exists:false}:await invoke('destination',{output});const overwrite=dest.exists&&confirm(i18n.t('同名CSVがあります。正常終了後に置き換えますか？'));if(dest.exists&&!overwrite)return;await invoke('begin_export',{profile:profile(),output,overwrite,splitOptions:doSplit?{limit:Number($('export-limit').value),header:true,count_header:$('export-count').checked,delimiter:',',encoding:'utf8'}:null});clearActionError('run-status');clearActionError('cancel');running=true;cancelRequested=false;started=Date.now();$('cancel').disabled=false;$('open-output').hidden=true;setText($('result'),'');setText($('run-status'),'接続・件数確認中');notify('出力を開始しました。');timer=setInterval(poll,350)});
async function poll(){try{const s=await invoke('status');clearActionError('run-status');$('received').textContent=format(s.received_bytes);setText($('elapsed'),`${((Date.now()-started)/1000).toFixed(1)} 秒`);if(s.running){if(s.received_bytes&&!cancelRequested)setText($('run-status'),'受信・保存・検証中');return}clearInterval(timer);running=false;$('cancel').disabled=true;const c=s.completion;if(c?.status==='complete'){const r=c.result;setText($('run-status'),'完了');setText($('result'),`${r.rows.toLocaleString()}行 / ${format(r.bytes)} / ${r.total_seconds.toFixed(2)}秒\n保存先：${r.output}`);if(r.files!==undefined){setText($('result'),`${r.rows.toLocaleString()}件 / ${r.files}ファイル / ${r.total_seconds.toFixed(2)}秒\n保存先：${r.output||''}`)}$('open-output').hidden=r.empty===true;notify(r.empty?'データがないため分割ファイルを作成しませんでした。':'CSVの保存と検証が完了しました。')}else{setText($('run-status'),'未完了');setText($('result'),'詳細を表示');notify('出力は完了していません。設定とエラーを確認して再試行してください。',true)}show();if(c?.status!=='complete')actionError('run-status',c?.error||'処理結果を取得できませんでした。')}catch(e){actionError('run-status',e)}}
$('cancel').onclick=async()=>{clearActionError('cancel');cancelRequested=true;refreshButtons();try{await invoke('cancel');setText($('run-status'),'中止処理中')}catch(e){cancelRequested=false;actionError('cancel',e)}finally{refreshButtons()}};$('open-output').onclick=()=>taskFor('open-output',()=>invoke('open_output'));
function rowIncrement(rows){
 return Number.isFinite(rows)&&rows>=100?10**Math.max(0,Math.floor(Math.log10(rows))-1):1;
}
function stepperRows(prefix){return prefix==='export'?exportEstimate?.rows:scanResult?Math.max(0,scanResult.records-Number($('split-header').checked)):null}
function updateStepper(prefix){
 const amount=rowIncrement(stepperRows(prefix)),ja=i18n.language==='ja';
 $(prefix+'-increment').textContent=ja?`矢印：${amount.toLocaleString()}行ずつ（手入力は1行単位）`:`Arrows: ${amount.toLocaleString()} rows per click (type any whole number)`;
 for(const [suffix,verb]of [['increase',ja?'増やす':'Increase'],['decrease',ja?'減らす':'Decrease']])$(prefix+'-'+suffix).setAttribute('aria-label',`${verb}: ${amount.toLocaleString()}`);
}
function adjustRows(prefix,direction){
 const input=$(prefix+'-limit');if(input.disabled)return;
 const minimum=Number(input.min),current=Number(input.value),amount=rowIncrement(stepperRows(prefix));
 input.value=String(Math.max(minimum,Math.min(Number.MAX_SAFE_INTEGER,(Number.isSafeInteger(current)?current:minimum)+direction*amount)));
 input.dispatchEvent(new Event('input',{bubbles:true}));
}
for(const prefix of ['export','split']){
 $(prefix+'-increase').onclick=()=>adjustRows(prefix,1);$(prefix+'-decrease').onclick=()=>adjustRows(prefix,-1);
 $(prefix+'-limit').addEventListener('keydown',event=>{if(event.key==='ArrowUp'||event.key==='ArrowDown'){event.preventDefault();adjustRows(prefix,event.key==='ArrowUp'?1:-1)}});
}
function syncSplitControls(){
 updateStepper('export');updateStepper('split');
 renderExportInfo();
 renderCsvInfo();
 $('scan-refresh').disabled=busy||running||!$('split-input').value;
 $('scan-cancel').hidden=!scanActive;
 $('scan-cancel').disabled=false;
 $('split-count').disabled=busy||running||!$('split-header').checked;
 $('split-limit').min=$('split-header').checked&&$('split-count').checked?'2':'1';
 $('export-limit').min=$('export-count').checked?'2':'1';
 $('export-split-options').hidden=!$('export-split').checked;
 $('split-execute').disabled=busy||running||!$('split-input').value||!$('split-parent').value||!splitValid($('split-limit').value,$('split-header').checked,$('split-count').checked);
}
$('operation').onchange=()=>{localMode=$('operation').value==='local';step=0;notify(localMode?'元ファイルを保持し、新しいフォルダーに分割結果を保存します。':'接続設定を入力するか、登録済み設定を読み込んでください。');show()};
$('split-again').onclick=()=>{step=0;show()};
function parentDestination(path){
 const index=Math.max(path.lastIndexOf('/'),path.lastIndexOf('\\'));
 if(index<0)return '.';
 return index===0||/^[A-Za-z]:[/\\]/.test(path)&&index===2?path.slice(0,index+1):path.slice(0,index);
}
function fileDestination(folder){const separator=folder.includes('\\')?'\\':'/';return folder.replace(/[/\\]+$/,'')+separator+'sensors.csv'}
$('export-split').onchange=()=>{
 const split=$('export-split').checked;if(split===destinationMode)return;
 const previous=$('output').value;destinations[destinationMode?'folder':'file']=previous;
 const key=split?'folder':'file';
 $('output').value=destinations[key]||(previous?(split?parentDestination(previous):fileDestination(previous)):'');
 destinationMode=split;clearDestinationEstimate();refreshButtons();summary();
};
for(const id of ['split-input','split-parent','split-header','split-count','split-limit','export-limit','export-count'])$(id).addEventListener('input',refreshButtons);
$('pick-csv').onclick=()=>taskFor('pick-csv',async()=>{const path=await invoke('choose',{kind:'csv'});if(path){$('split-input').value=path;scheduleScan(0)}});
$('pick-parent').onclick=()=>taskFor('pick-parent',async()=>{const path=await invoke('choose',{kind:'folder'});if(path)$('split-parent').value=path});
$('split-execute').onclick=()=>taskFor('split-execute',async()=>{
 stopScan();
 const options={limit:Number($('split-limit').value),header:$('split-header').checked,count_header:$('split-count').checked,encoding:$('split-encoding').value,delimiter:$('split-delimiter').value==='tab'?'\t':$('split-delimiter').value};
 await invoke('begin_split',{input:$('split-input').value,output:$('split-parent').value,options});
 clearActionError('run-status');clearActionError('cancel');running=true;cancelRequested=false;started=Date.now();step=4;$('open-output').hidden=true;setText($('result'),'');setText($('run-status'),'受信・保存・検証中');timer=setInterval(poll,350);
});

function renderCsvInfo(){
 updateStepper('split');
 const ja=i18n.language==='ja', labels=ja?['ファイル名','ファイルサイズ','データ件数（ヘッダー除く）','ヘッダー','分割後のファイル数','1ファイルのデータ上限']:['File name','File size','Data records (excluding header)','Header','Files after splitting','Data records per file'];
 const head=$('split-header').checked,limit=Number($('split-limit').value),valid=splitValid(limit,head,$('split-count').checked);
 const capacity=valid?limit-Number(head&&$('split-count').checked):null;
 const rows=scanResult?Math.max(0,scanResult.records-Number(head)):null;
 const values=[scanMeta?.name||'—',scanMeta?`${format(scanMeta.bytes)} (${scanMeta.bytes.toLocaleString()} bytes)`:'—',rows===null?'—':rows.toLocaleString(),head?(ja?'あり（利用者指定）':'Present (selected)'):(ja?'なし（利用者指定）':'None (selected)'),rows===null||!valid?'—':Math.ceil(rows/capacity).toLocaleString(),capacity===null?'—':capacity.toLocaleString()+(head?(ja?' ＋ ヘッダー1行':' + 1 header record'):'')];
 $('csv-info-values').replaceChildren();labels.forEach((label,i)=>{const dt=document.createElement('dt'),dd=document.createElement('dd');dt.textContent=label;dd.textContent=values[i];$('csv-info-values').append(dt,dd)});
 const states=ja?{idle:'CSVを選択すると件数を集計します。',scanning:'件数を集計中です。設定の変更や中止ができます。',complete:'集計完了。ヘッダー設定と指定行数から分割数を計算しています。',cancelled:'集計を中止しました。再集計できます。',error:'集計できませんでした。形式と詳細を確認してください。'}:{idle:'Select a CSV to count its records.',scanning:'Counting records. You can change settings or stop counting.',complete:'Count complete. File count uses your header and row-limit settings.',cancelled:'Counting stopped. You can scan again.',error:'Could not count records. Check the format and details.'};
 $('csv-scan-status').textContent=states[scanState];
}
function stopScan(){
 clearTimeout(scanTimer);const id=scanId++;if(scanActive)scanState='cancelled';if(scanActive)invoke('cancel_csv_scan',{id}).catch(e=>{if(scanId===id+1)actionError('scan-refresh',e)});scanActive=false;
}
function scheduleScan(delay=250){
 stopScan();scanResult=null;scanMeta=null;clearActionError('scan-refresh');scanState=$('split-input').value?'scanning':'idle';refreshButtons();
 if($('split-input').value)scanTimer=setTimeout(()=>scanCsv(),delay);
}
async function scanCsv(){
 stopScan();const id=scanId,input=$('split-input').value;if(!input)return;
 scanActive=true;scanState='scanning';scanResult=null;clearActionError('scan-refresh');refreshButtons();
 const options={limit:1,header:false,count_header:false,encoding:$('split-encoding').value,delimiter:$('split-delimiter').value==='tab'?'\t':$('split-delimiter').value};
 try{
  const meta=await invoke('csv_metadata',{input});if(id!==scanId)return;scanMeta=meta;renderCsvInfo();
  const result=await invoke('scan_csv',{input,options,id});if(id!==scanId)return;scanResult=result;scanMeta.bytes=result.bytes;scanState='complete';
 }catch(e){if(id!==scanId)return;scanState='error';actionError('scan-refresh',e)}finally{if(id===scanId){scanActive=false;refreshButtons()}}
}
for(const id of ['split-input','split-encoding','split-delimiter'])$(id).addEventListener('input',()=>scheduleScan());
$('scan-refresh').onclick=()=>scanCsv();
$('scan-cancel').onclick=()=>{stopScan();scanResult=null;scanState='cancelled';refreshButtons()};
function validateConnection(){
 connectionFields(profile());const port=$('ssh_port').value;
 if(port&&(!/^\d+$/.test(port)||Number(port)<1||Number(port)>65535))throw new Error('SSHポートは1から65535の整数にしてください。');
 if($('auth').value==='key'&&!$('identity_file').value.trim())throw new Error('秘密鍵ファイルを指定してください。');
}
function applyConnection(c){
 $('ssh_host').value=c.host;$('ssh_user').value=c.user;$('ssh_port').value=c.port;$('auth').value=c.auth_mode;$('identity_file').value=c.identity_file;
 $('ssh_secret').value='';$('dbpass').value='';$('save-secrets').checked=false;$('ssh-command').value='';invalidate();visibility();
}
function connectionSummary(){
 const fallback=i18n.t('SSH設定を使用');$('ssh-summary').textContent=`${$('ssh_user').value||fallback} @ ${$('ssh_host').value||'—'} · ${$('ssh_port').value||fallback} · ${i18n.t($('auth').selectedOptions[0]?.dataset.i18nOriginal||$('auth').selectedOptions[0]?.textContent||'')}`;
}
function renderHistory(){
 const selected=$('history-list').value;const query=$('history-search').value.toLowerCase();$('history-list').replaceChildren();
 historyRows.forEach((r,index)=>{if(!r.ssh_host.toLowerCase().includes(query))return;const option=document.createElement('option');option.value=String(index);option.textContent=`${r.ssh_host} · ${r.ssh_port||i18n.t('SSH設定を使用')} · ${r.auth_mode} · ${new Date(r.last_success*1000).toLocaleString(i18n.language==='ja'?'ja-JP':'en-US')}`;$('history-list').append(option)});
 if([...$('history-list').options].some(o=>o.value===selected))$('history-list').value=selected;
}
async function loadHistory(){try{const h=await invoke('connection_history',{action:'read',value:null});historyRows=h.entries||[];$('history-enabled').checked=h.enabled!==false;setText($('history-warning'),'');renderHistory();}catch{historyRows=[];renderHistory();setText($('history-warning'),'履歴を読み込めません。手入力するか、履歴をすべて削除してください。');}}
$('apply-ssh').onclick=()=>taskFor('apply-ssh',async()=>{const c=parseSSH($('ssh-command').value);applyConnection(c)});
$('ssh-command').onkeydown=e=>{if(e.key==='Enter'){e.preventDefault();$('apply-ssh').click();}};
$('history-search').oninput=renderHistory;
$('history-load').onclick=()=>taskFor('history-load',async()=>{const r=historyRows[Number($('history-list').value)];if($('history-list').value!==''&&r)applyConnection(connectionFields(r));});
$('history-delete').onclick=()=>taskFor('history-delete',async()=>{const r=historyRows[Number($('history-list').value)];if($('history-list').value!==''&&r){await invoke('connection_history',{action:'delete',value:r});await loadHistory();}});
$('history-clear').onclick=()=>taskFor('history-clear',async()=>{if(confirm(i18n.t('接続履歴をすべて削除しますか？登録設定は削除しません。'))){await invoke('connection_history',{action:'clear',value:null});await loadHistory();}});
$('history-enabled').onchange=()=>taskFor('history-enabled',async()=>{try{await invoke('connection_history',{action:'enabled',value:$('history-enabled').checked});}finally{await loadHistory();}});
visibility();show();loadHistory();saved().catch(e=>actionError('load',e));

invoke('language_preference',{value:null}).then(value=>{if(languageChanged)return;if(['en','ja'].includes(value)){i18n.change(value);$('language').value=value;visibility();show();}else if(value===null){return invoke('language_preference',{value:i18n.language});}}).catch(()=>{});
