import {test} from 'node:test';
import assert from 'node:assert/strict';
import {canAdvance,invalidateConnection,periodValid} from './state.mjs';
test('接続変更で古い成功と対象を解除',()=>assert.deepEqual(invalidateConnection({ssh:true,db:true,table:'old'}),{ssh:false,db:false,table:null}));
test('未確認の接続と列なしで先に進めない',()=>{assert.equal(canAdvance(0,{ssh:true,db:false}),false);assert.equal(canAdvance(2,{columns:0,periodValid:true}),false)});
test('期間逆転と同一境界を拒否',()=>{assert.equal(periodValid('2026-10-02','2026-10-01'),false);assert.equal(periodValid('2026-10-01','2026-10-01'),false);assert.equal(periodValid('','2026-10-01'),true)});
