import {readFileSync} from 'node:fs';
import test from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
test('audio call uses the peer selected at the click',()=>{
  const source=readFileSync(new URL('../src/main.js',import.meta.url),'utf8');
  const start=source.indexOf('$("call-button")?.addEventListener');
  const end=source.indexOf('$("chat-search-input")',start);
  let click;const commands=[];const state={current:'1'.repeat(64)};
  const context=vm.createContext({state,$:()=>({addEventListener:(_event,fn)=>{click=fn}}),tr:value=>value,
    toast:()=>{},audioCalls:{start:peer=>commands.push({recipient_device:peer})}});
  vm.runInContext(source.slice(start,end),context);
  click({preventDefault(){}});state.current='2'.repeat(64);
  assert.equal(commands[0].recipient_device,'1'.repeat(64));
});
