export const STORAGE_KEY='pgcsv.language';
export function readLanguage(storage){try{return storage.getItem(STORAGE_KEY)==='ja'?'ja':'en'}catch{return 'en'}}
export function saveLanguage(storage,language){try{storage.setItem(STORAGE_KEY,language)}catch{/* Selection still works when storage is unavailable. */}}
export const messages={
 'サーバーのデータを、':'Server data,', '手元のCSVへ。':'your local CSV.',
 '接続設定':'Connection', '出力対象':'Source', '出力範囲':'Scope', '保存設定':'Destination', '実行・結果':'Export & results',
 '1回につき1テーブル':'One table per export', 'UTF-8・ヘッダー付き':'UTF-8 with headers', '読取り専用で出力':'Read-only export',
 'Windows デスクトップ':'Windows desktop', '接続設定を入力するか、登録済み設定を読み込んでください。':'Enter connection details or load a saved configuration.',
 'サーバーへの接続と、PostgreSQLへの接続を順番に確認します。':'Check the server connection first, then PostgreSQL.',
 '登録済み設定':'Saved configuration', '新しい設定':'New configuration', '読み込む':'Load', '削除':'Delete', 'JSONから読み込む':'Import JSON',
 'サーバーへの接続':'Server connection', '接続先・SSH設定名':'Host or SSH alias', '例：server-alias':'e.g. server-alias',
 'ユーザー名（任意）':'Username (optional)', 'SSHポート（未入力なら既存設定）':'SSH port (blank: existing configuration)', '認証方法':'Authentication',
 '既存SSH設定':'Existing SSH configuration', '秘密鍵':'Private key', 'パスワード':'Password', '秘密鍵ファイル':'Private key file', '選択':'Browse',
 'サーバーの出力環境':'Server export environment', 'Python実行ファイル（psycopg導入済み）':'Python executable (with psycopg)',
 'サーバーにあるPython環境を利用します。ソフトウェアを自動インストールしません。':'Uses the server’s Python environment. Software is not installed automatically.',
 'サーバー接続を確認':'Check server connection', '未確認':'Not checked', 'PostgreSQLへの接続':'PostgreSQL connection',
 '接続設定の指定方法':'Connection configuration', 'サーバーの既存設定ファイル':'Existing server configuration file', 'DB接続情報を入力':'Enter database connection details',
 'サーバー上の設定ファイル':'Configuration file on server', '例：/etc/myapp/server.env':'e.g. /etc/myapp/server.env', '接続文字列の変数名':'Connection string variable',
 'DBホスト':'Database host', 'DBポート':'Database port', '初期データベース':'Initial database', 'DBユーザー':'Database user', 'DBパスワード':'Database password', 'DB接続を確認':'Check database connection',
 '今回出力する1つのテーブルを選択します。':'Select one table to export.', 'データベース':'Database', 'スキーマ':'Schema', 'テーブル':'Table', '一覧を更新':'Refresh list',
 'プレビュー':'Preview', '最大10行':'Up to 10 rows', 'データを確認':'Preview data', 'テーブルを選んで確認してください。':'Select a table, then preview its data.',
 '必要な列と期間を指定します。プレビューの10行制限は本出力に適用しません。':'Choose columns and a date range. The preview’s 10-row limit does not apply to exports.',
 '出力する列':'Columns to export', 'すべて選択':'Select all', '選択を解除':'Clear selection', '対象期間':'Date range', '期間':'Range', '全期間':'All dates', '期間を指定':'Custom range',
 '日時列':'Date/time column', '開始日時（この日時を含む）':'Start (inclusive)', '終了日時（この日時を含まない）':'End (exclusive)',
 'タイムゾーン付き日時の指定は日本時間です。タイムゾーンのない列は元の日時で比較します。':'Timezone-aware timestamps use Japan Standard Time (JST). Columns without a timezone use their original date/time values.',
 '保存設定・確認':'Destination & review', '出力内容と保存先を確認してから実行します。':'Review the export scope and destination before starting.', 'CSVの保存先':'CSV destination', '保存先を選択':'Choose destination',
 '件数・容量を見積もる':'Estimate rows & size', '見積もりにはDBへの読取りが発生します。':'Estimation reads from the database.', 'この設定を次回も使う':'Reuse this configuration',
 '設定名':'Configuration name', '例：センサーデータ全件':'e.g. All sensor data', 'パスワードをWindows資格情報ストアへ保存する':'Save passwords in Windows Credential Manager',
 'オフの場合、パスワードは設定ファイルへ保存せず、次回入力します。':'When unchecked, passwords are not saved in the configuration. Enter them again next time.', '設定を登録・更新':'Save configuration',
 '接続、受信・保存、検証、完了の順に処理します。':'Connect, receive and save, validate, then complete.', '実行準備ができました。':'Ready to export.', '受信済み':'Received', '経過時間':'Elapsed',
 'CSV出力':'Export CSV', 'キャンセル':'Cancel', '保存先を開く':'Open destination', '処理中は設定を変更できません。閉じる場合は先にキャンセルしてください。':'Settings are locked during export. Cancel before closing the application.',
 '戻る':'Back', '次へ':'Next', '接続先のホスト鍵を確認':'Verify server host key', '管理者などの信頼できる情報と、次の指紋を照合してください。':'Compare this fingerprint with trusted information from your administrator.',
 '接続を中止':'Abort connection', '指紋を確認して接続':'Verify fingerprint & connect',
 'ホスト鍵の確認を中止しました。':'Host key verification was cancelled.', 'ホスト鍵が一致しません。接続を中止しました。':'The host key does not match. Connection aborted.',
 '条件変更後は、必要に応じて再度見積もってください。':'The scope changed. Estimate again if needed.', '対象':'Source', '接続先DB':'Connected database', '列':'Columns', '指定なし':'Not set',
 '形式':'Format', 'UTF-8 BOM・ヘッダーあり・日本時間':'UTF-8 BOM, headers, Japan Standard Time (JST)', '保存先':'Destination',
 '秘密鍵のパスフレーズ（任意）':'Private key passphrase (optional)', 'SSHパスワード':'SSH password', '接続・出力環境を確認済み':'Connection & export environment verified',
 'サーバーへの接続と出力環境を確認しました。次にDB接続を確認してください。':'Server connection and export environment verified. Check the database connection next.',
 '接続済み':'Connected', 'DB接続を確認しました。出力対象を選択してください。':'Database connection verified. Select the export source.',
 '設定を読み込みました。接続を確認してください。':'Configuration imported. Check the connections.', '登録済み設定を読み込みました。接続を確認してください。':'Saved configuration loaded. Check the connections.',
 '設定を削除しました。':'Configuration deleted.', '設定を登録しました。':'Configuration saved.', '保存先を指定してください。':'Choose a destination.',
 '同名CSVがあります。正常終了後に置き換えますか？':'A CSV with this name exists. Replace it after a successful export?', '接続・件数確認中':'Connecting & checking row count',
 '出力を開始しました。':'Export started.', '受信・保存・検証中':'Receiving, saving & validating', '完了':'Complete', 'CSVの保存と検証が完了しました。':'CSV saved and validated.',
 '未完了':'Incomplete', '処理結果を取得できませんでした。':'Could not retrieve the export result.', '出力は完了していません。設定とエラーを確認して再試行してください。':'Export did not complete. Check the settings and error, then retry.', '中止処理中':'Cancelling',
 '同じフォルダーにpgcsv.exeが必要です。':'pgcsv.exe is required in the same folder.', '設定ファイルが大きすぎます。':'The configuration file is too large.', '設定はJSON objectにしてください。':'Configuration must be a JSON object.',
 '設定がありません。':'Configuration not found.', '保存済み資格情報を取得できません。':'Could not retrieve saved credentials.', '設定名を入力してください。':'Enter a configuration name.', '資格情報の保存に失敗しました。':'Could not save credentials.',
 '保存先に書き込めません。':'Cannot write to the destination.', '完了したCSVがありません。':'No completed CSV is available.', '保存先がありません。':'Destination not found.', '実行中です。':'An export is running.'
};
export const patterns=[
 ['{0}行・CSV推定 {1}（最大1,000行から推定） / 保存先の空き {2} / 空き容量不足の可能性があります。','{0} rows · estimated CSV {1} (sample: up to 1,000 rows) / free space {2} / There may not be enough free space.'],
 ['{0}行・CSV推定 {1}（最大1,000行から推定） / 保存先の空き {2}','{0} rows · estimated CSV {1} (sample: up to 1,000 rows) / free space {2}'],
 ['{0}行を表示しました。プレビューは各値1024文字まで。本出力には制限を適用しません。','Showing {0} rows. Preview values are limited to 1024 characters. Exports are not truncated.'],
 ['設定「{0}」を削除しますか？','Delete configuration “{0}”?'],
 ['{0} 以上 ～ {1} 未満','From {0} (inclusive) to {1} (exclusive)'],
 ['{0}行・CSV推定 {1}（最大1,000行から推定）','{0} rows · estimated CSV {1} (sample: up to 1,000 rows)'],
 [' / 保存先の空き {0}',' / free space {0}'],
 [' / 空き容量不足の可能性があります。',' / There may not be enough free space.'],
 ['{0} 秒','{0} s'], ['{0}行 / {1} / {2}秒\n保存先：{3}','{0} rows / {1} / {2} s\nDestination: {3}'],
 ['アプリ内で開いてください。{0}','Open this page in the application. {0}']
];
const escape=s=>s.replace(/[.*+?^${}()|[\]\\]/g,'\\$&');
const compiled=patterns.map(([ja,en])=>({re:new RegExp('^'+ja.split(/\{\d+\}/).map(escape).join('([\\s\\S]*?)')+'$'),en}));
export function translate(text,language){
 if(language!=='en')return text;
 if(Object.hasOwn(messages,text))return messages[text];
 for(const {re,en} of compiled){const match=re.exec(text);if(match)return en.replace(/\{(\d+)\}/g,(_,i)=>match[Number(i)+1]);}
 return text;
}
export function createTranslator(doc,storage){
 let language=readLanguage(storage);
 const texts=new Map(),attributes=[];
 const walk=doc.createTreeWalker(doc.body,4);
 for(let node; (node=walk.nextNode());){if(node.parentElement?.closest('[data-no-i18n],script,style'))continue;const trimmed=node.data.trim();if(/[ぁ-んァ-ン一-龠]/.test(trimmed))texts.set(node,node.data);}
 for(const element of doc.querySelectorAll('[placeholder]'))attributes.push([element,element.getAttribute('placeholder')]);
 function apply(){doc.documentElement.lang=language;for(const [node,source]of texts){if(!node.isConnected){texts.delete(node);continue;}const value=source.trim();node.data=source.replace(value,translate(value,language));}for(const [element,source]of attributes)element.setAttribute('placeholder',translate(source,language));}
 function setText(element,source){for(const node of element.childNodes)texts.delete(node);element.textContent=translate(String(source),language);if(element.firstChild)texts.set(element.firstChild,String(source));}
 function change(value){language=value==='ja'?'ja':'en';saveLanguage(storage,language);apply();}
 apply();return {setText,change,t:text=>translate(String(text),language),get language(){return language}};
}
