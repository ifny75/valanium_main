import {readFileSync} from 'node:fs';
import assert from 'node:assert/strict';
import test from 'node:test';
const {createUpdater}=await import('data:text/javascript;base64,'+Buffer.from(readFileSync(new URL('../src/updater.js',import.meta.url),'utf8')).toString('base64'));
function setup(invoke,prepareTor=async()=>{}){
  const status={textContent:''};const button={disabled:false,hidden:true,classList:{toggle(_name,value){button.hidden=value}},addEventListener(_name,fn){button.click=fn}};
  return {status,button,updater:createUpdater({invoke,version:async()=>'0.13.14',isOnion:()=>true,prepareTor,status,button,notify:()=>{},tr:text=>text})};
}
test('Tor is ready before native download; installation requires its own click',async()=>{
  const calls=[];const {updater,button}=setup(async(name,args)=>{calls.push([name,args]);return {ready:true,version:'0.13.15'}},async()=>{calls.push(['tor'])});
  await updater.check();assert.deepEqual(calls.map(x=>x[0]),['tor','prepare_update']);assert.equal(calls[1][1].onion,true);
  assert.equal(button.hidden,false);await button.click();assert.equal(calls.at(-1)[0],'install_update');
});
test('a failed Tor bootstrap never makes an HTTPS request',async()=>{
  let requests=0;const {updater,button}=setup(async()=>{requests++},async()=>{throw Error('Tor unavailable')});
  await updater.check();assert.equal(requests,0);assert.equal(button.hidden,true);assert.equal(button.disabled,false);
});
test('concurrent checks share one download and an absent update cannot be installed',async()=>{
  let finish,requests=0;const pending=new Promise(resolve=>{finish=resolve});
  const {updater,button}=setup(async()=>{requests++;return pending});
  const first=updater.check();await Promise.resolve();await updater.check();finish({ready:false});await first;await button.click();
  assert.equal(requests,1);assert.equal(button.hidden,true);
});
