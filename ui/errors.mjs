import catalog from './error-catalog.mjs';
export function describeError(error,operation,language,secrets=[]){
 let detail=error?.detail||error?.message||String(error);
 for(const secret of secrets.filter(v=>typeof v==='string'&&v.length).sort((a,b)=>b.length-a.length)){detail=detail.split(secret).join('[redacted]').split(encodeURIComponent(secret)).join('[redacted]');}
 detail=detail.replace(/postgres(?:ql)?:\/\/[^\s]+/gi,'[database connection redacted]').replace(/(password|passphrase|token)\s*[=:]\s*(?:"[^"]*"|'[^']*'|[^\s,;]+)/gi,'$1=[redacted]');
 const embedded=/^(ODC-[A-Z]+-\d{3}):/.exec(detail)?.[1];
 const candidate=error?.code||embedded;
 const fallback=operation==='test-ssh'?'ODC-SSH-001':operation==='apply-ssh'?'ODC-CONFIG-001':['test-db','database_name','refresh','preview','estimate'].includes(operation)?'ODC-DB-001':['execute','split-execute','pick-key','pick-output','pick-csv','pick-parent','open-output','run-status'].includes(operation)?'ODC-SAVE-001':operation.startsWith('history-')||['load','save','remove','import'].includes(operation)?'ODC-HISTORY-001':'ODC-UNKNOWN-001';
 const code=Object.hasOwn(catalog,candidate)?candidate:fallback;
 return {code,...catalog[code][language==='ja'?'ja':'en'],detail:detail.slice(0,16000),url:'https://github.com/ours-labs/data-converter/blob/main/docs/ERRORS.md#'+code.toLowerCase()};
}
