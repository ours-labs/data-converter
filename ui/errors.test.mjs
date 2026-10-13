import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync,existsSync} from 'node:fs';
import catalog from './error-catalog.mjs';
import {describeError} from './errors.mjs';
test('all codes have bilingual summaries, offline advice and matching public reference',()=>{
 for(const [code,item] of Object.entries(catalog)){
  assert.match(code,/^ODC-[A-Z]+-\d{3}$/);
  for(const lang of ['ja','en']){assert.ok(item[lang].summary);assert.ok(item[lang].action);const privatePath=new URL(`../docs/public/ERRORS${lang==='ja'?'.ja':''}.md`,import.meta.url);const doc=readFileSync(existsSync(privatePath)?privatePath:new URL(`../docs/ERRORS${lang==='ja'?'.ja':''}.md`,import.meta.url),'utf8');assert.ok(doc.includes('## '+code));assert.ok(doc.includes(item[lang].summary));}
 }
});
test('known codes stay stable; unknown errors do not invent authentication diagnoses',()=>{
 assert.equal(describeError('ODC-DB-002: SQLSTATE=28P01','test-db','ja').code,'ODC-DB-002');
 assert.equal(describeError('password failed?','test-db','en').code,'ODC-DB-001');
 assert.equal(describeError({code:'unknown',detail:'unknown'},'cancel','ja').code,'ODC-UNKNOWN-001');
});
test('secret values and connection URLs are excluded from details and help URLs',()=>{
 const result=describeError('postgresql://user:pass@host/db secret1 password="another secret"','test-db','en',['secret1']);
 assert.ok(!result.detail.includes('secret1'));assert.ok(!result.detail.includes('user:pass'));assert.ok(!result.detail.includes('another secret'));
 assert.equal(result.url,'https://github.com/ours-labs/data-converter/blob/main/docs/ERRORS.md#odc-db-001');
});
