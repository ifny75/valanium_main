import {readFileSync} from 'node:fs';
import test from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
test('call confirmation keeps the peer selected when the dialog opened',()=>{
  const source=readFileSync(new URL('../src/main.js',import.meta.url),'utf8');
  const start=source.indexOf('$("call-button")?.addEventListener');
  const end=source.indexOf('$("chat-search-input")',start);
  let click,confirm;const commands=[];const state={current:'1'.repeat(64)};
  const context=vm.createContext({state,$:()=>({addEventListener:(_event,fn)=>{click=fn}}),tr:value=>value,
    toast:()=>{},confirmAction:(_title,_detail,fn)=>{confirm=fn},submit:command=>commands.push(command)});
  vm.runInContext(source.slice(start,end),context);
  click({preventDefault(){}});state.current='2'.repeat(64);confirm();
  assert.equal(commands[0].recipient_device,'1'.repeat(64));
});
