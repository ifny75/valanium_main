import {readFileSync} from 'node:fs';
import test from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
const source=readFileSync(new URL('../src/main.js',import.meta.url),'utf8');
const start=source.indexOf('function settingsMatches('),end=source.indexOf('// Search all settings',start);
const context=vm.createContext({getLocale:()=> 'en-US'});vm.runInContext(source.slice(start,end),context);
test('settings search matches word prefixes without treating stored as Tor',()=>{
  assert.equal(context.settingsMatches('Tor Default','tor'),true);
  assert.equal(context.settingsMatches('Stored only on this device','tor'),false);
  assert.equal(context.settingsMatches('Сквозное шифрование','шифр'),true);
  assert.equal(context.settingsMatches('Private identity keys','keys private'),true);
  assert.equal(context.settingsMatches('Private identity keys','password'),false);
});
test('Escape clears settings search without reaching the window close handler',()=>{
  let handler,prevented=false,stopped=false,cleared=false;
  const input={value:'Tor',addEventListener:(_name,callback)=>{handler=callback}};
  const results={replaceChildren:()=>{cleared=true}};
  const keyboard=vm.createContext({$:id=>id==='settings-search'?input:results});
  const from=source.indexOf('$("settings-search").addEventListener("keydown"');
  const to=source.indexOf('$("settings-nav").addEventListener("keydown"',from);
  vm.runInContext(source.slice(from,to),keyboard);
  handler({key:'Escape',preventDefault:()=>{prevented=true},stopPropagation:()=>{stopped=true}});
  assert.equal(input.value,'');assert.ok(prevented&&stopped&&cleared);
});
