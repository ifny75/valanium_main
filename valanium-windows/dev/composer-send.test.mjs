import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
const source = readFileSync(new URL('../src/main.js', import.meta.url), 'utf8').replace(/\r\n/g,'\n');
const html = readFileSync(new URL('../src/index.html', import.meta.url), 'utf8');
function between(a,b) { const i=source.indexOf(a),j=source.indexOf(b,i+a.length); assert.ok(i>=0&&j>i);return source.slice(i,j); }
function listener(id,event) { const a=`$("${id}").addEventListener("${event}"`;const i=source.indexOf(a),j=source.indexOf('\n});',i);assert.ok(i>=0&&j>i);return source.slice(i,j+4); }
function fixture() {
  const elements=Object.fromEntries([...html.matchAll(/\bid="([^"]+)"/g)].map(m=>[m[1],{
    value:'', textContent:'', style:{}, scrollHeight:24, scrollTop:0, classList:{ add(){},remove(){} },
    closest:()=>({style:{setProperty(){}}}), addEventListener(type,fn){this[type]=fn;}
  }]));
  const sent=[],appended=[];
  const scope=vm.createContext({ $:id=>elements[id], state:{current:'test-device'},
    editing:null, replyingTo:null, preferences:{sendKey:'enter'},
    isGroupKey:()=>false, logicalId:()=> 'logical-qa', conversationOf:()=> 'test-conversation',
    encodeContent:value=>JSON.stringify(value), submit:async command=>{sent.push(command);return true;},
    appendMessage:(...args)=>appended.push(args), renderReplyBar(){}, renderEditBar(){} });
  vm.runInContext(between('function calculateExpression(', '$("composer").addEventListener("input", updateComposerCalculation)'),scope);
  vm.runInContext(between('function resizeComposer()', '$("composer").addEventListener("input", resizeComposer)'),scope);
  vm.runInContext(listener('form-send','submit'),scope);
  vm.runInContext(listener('composer','keydown'),scope);
  elements['form-send'].requestSubmit=()=>{elements.pending=elements['form-send'].submit({preventDefault(){}});};
  return {elements,scope,sent,appended};
}
test('real composer markup supports typing and dispatches a send command on submit',async()=>{
  const {elements,scope,sent,appended}=fixture();
  elements.composer.value='Hello from desktop';
  vm.runInContext('updateComposerCalculation()',scope);
  await elements['form-send'].submit({preventDefault(){}});
  assert.equal(sent.length,1);
  assert.equal(sent[0].type,'send');
  assert.equal(JSON.parse(sent[0].body).text,'Hello from desktop');
  assert.equal(elements.composer.value,'');
  assert.equal(appended.length,1);
});
test('Enter submits through the same form; calculator output does not break sending',async()=>{
  const {elements,scope,sent}=fixture();
  elements.composer.value='2+2=';
  vm.runInContext('updateComposerCalculation()',scope);
  assert.equal(elements['composer-calculation'].textContent,'4');
  elements.composer.keydown({key:'Enter',preventDefault(){}});
  await elements.pending;
  assert.equal(sent.length,1);
  assert.equal(JSON.parse(sent[0].body).text,'2+2=');
});
