// Parse a deliberately small SSH grammar; never execute the entered string.
const invalid=()=>{throw new Error('SSH入力を確認してください。対応形式: ssh [-p port] [-i key] [-l user] [user@]host');};
export function destination(value){
 if(typeof value!=='string'||!value||value.length>512||/[^A-Za-z0-9_.:@%\[\]-]/.test(value))invalid();
 const parts=value.split('@');if(parts.length>2)invalid();
 const user=parts.length===2?parts[0]:'';let host=parts.at(-1);
 if(parts.length===2&&!user||user.startsWith('-')||host.startsWith('-')||!host)invalid();
 if(user&&!/^[A-Za-z0-9_.-]+$/.test(user))invalid();
 if(host.includes(':')){host=host.replace(/^\[([^\]]+)\]$/,'$1');if(!/^[0-9A-Fa-f:]+(?:%[A-Za-z0-9_.-]+)?$/.test(host)||!host.includes('::')&&host.split(':').length!==8)invalid();try{new URL(`http://[${host.split('%')[0]}]/`);}catch{invalid();}}
 else if(!/^[A-Za-z0-9_][A-Za-z0-9_.-]*$/.test(host))invalid();
 return {host,user};
}
export function parseSSH(text){
 if(typeof text!=='string'||text.length>4096||/[\r\n\x00-\x1f;&|<>`$]/.test(text))invalid();
 const tokens=[];let token='',quote='',active=false;
 for(const c of text.trim()){
  if(quote){if(c===quote)quote='';else token+=c;active=true;}
  else if(c==='"'||c==="'"){quote=c;active=true;}
  else if(/\s/.test(c)){if(active){tokens.push(token);token='';active=false;}}
  else{token+=c;active=true;}
 }
 if(quote)invalid();if(active)tokens.push(token);const command=tokens[0]==='ssh';if(command)tokens.shift();
 let host='',user='',port='',key='';const seen=new Set();
 for(let i=0;i<tokens.length;i++){
  const t=tokens[i];if(t.startsWith('-')){
   if(!command||!['-p','-i','-l'].includes(t)||seen.has(t))invalid();seen.add(t);const v=tokens[++i];if(!v||v.startsWith('-'))invalid();
   if(t==='-p'){if(!/^\d+$/.test(v)||Number(v)<1||Number(v)>65535)invalid();port=String(Number(v));}
   if(t==='-i')key=v;if(t==='-l')user=v;
  }else{if(host)invalid();host=t;}
 }
 const d=destination(host);if(user&&d.user)invalid();if(user)destination(`${user}@${d.host}`);
 return {host:d.host,user:user||d.user,port,identity_file:key,auth_mode:key?'key':'existing'};
}
export function connectionFields(p){const d=destination(p.ssh_host||'');return {...d,port:p.ssh_port==null?'':String(p.ssh_port),identity_file:p.identity_file||'',auth_mode:p.auth_mode||(p.identity_file?'key':'existing')};}
