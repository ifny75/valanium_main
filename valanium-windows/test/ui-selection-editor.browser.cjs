const {chromium}=require('playwright');
const fs=require('node:fs');
const assert=require('node:assert/strict');
const path=require('node:path');
const http=require('node:http');
const root=path.resolve(__dirname,'../src');
const mime={'.html':'text/html','.js':'text/javascript','.css':'text/css','.svg':'image/svg+xml','.json':'application/json'};
const server=http.createServer((req,res)=>{
 const target=path.resolve(root,'.'+decodeURIComponent(new URL(req.url,'http://localhost').pathname));
 if(target!==root&&!target.startsWith(root+path.sep)){res.writeHead(403).end();return;}
 const file=target===root?path.join(root,'index.html'):target;
 fs.readFile(file,(error,bytes)=>{if(error){res.writeHead(404).end();return;}res.setHeader('Content-Type',mime[path.extname(file)]||'application/octet-stream');res.end(bytes);});
});
(async()=>{
 await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
 const origin=`http://127.0.0.1:${server.address().port}`;
 const executablePath=process.env.CHROMIUM_PATH || (fs.existsSync('/usr/bin/chromium')?'/usr/bin/chromium':undefined);
 const browser=await chromium.launch({executablePath,args:['--no-sandbox']});
 try {
 const page=await browser.newPage({viewport:{width:1440,height:1000}});
 const errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.addInitScript(()=>{
  localStorage.setItem('valanium.onboarding.language','ru');
  window.sent=[];
  window.__TAURI__={core:{invoke:async(name,args)=>{if(name==='submit')window.sent.push(JSON.parse(args.json));return name==='auto_unlock'?'locked':null;}},event:{listen:async()=>()=>{}}};
  navigator.clipboard.writeText=async text=>window.copied=text;
 });
 await page.route('**/main.js',route=>route.fulfill({contentType:'text/javascript',body:fs.readFileSync(path.join(root,'main.js'),'utf8')+'\nwindow.testUI={state,editor,bulkSelection,groupKey,showScreen,renderConversations,selectConversation,toggleBulkSelection,clearBulkSelection,openEditor,renderPhoto,buildMessage,encodeContent,preferences,applyPreferences,history};'}));
 await page.goto(origin);
 await page.waitForFunction(()=>window.testUI);
 await page.waitForTimeout(400);
 await page.evaluate(()=>{
  const t=window.testUI;t.state.device='owner';t.state.current='alice';
  t.state.conversations.set('alice',{conversation:'thread-a'});t.state.conversations.set('bob',{conversation:'thread-b'});
  t.showScreen('screen-main');t.renderConversations();
  const list=document.getElementById('messages');list.replaceChildren();
  for(const [id,text,outgoing] of [['a','Первое',true],['b','Второе',false]])list.append(t.buildMessage({outgoing,body:t.encodeContent({type:'text',id,text}),created_at:Date.now()},'alice').node);
  t.toggleBulkSelection('messages','b');t.toggleBulkSelection('messages','a');
 });
 await page.locator('#bulk-toolbar button').first().click();
 assert.equal(await page.evaluate(()=>window.copied),'Первое\nВторое');
 assert.equal(await page.locator('#bulk-toolbar').count(),0);
 await page.evaluate(()=>{testUI.toggleBulkSelection('chats','bob');testUI.toggleBulkSelection('chats','alice');});
 await page.getByRole('button',{name:'Закрепить / открепить',exact:true}).click();
 assert.deepEqual(await page.evaluate(()=>testUI.preferences.pinnedChats.owner),['bob','alice']);
 await page.evaluate(()=>{
  testUI.state.groups.set('group-c',{title:'Group C',kind:'group',members:[]});testUI.renderConversations();
 });
 await page.locator('#conversations li').filter({hasText:'Group C'}).getByRole('button').click({button:'right'});
 await page.locator('#bulk-toolbar button').first().click();
 assert.ok(await page.evaluate(()=>testUI.preferences.pinnedChats.owner.includes(testUI.groupKey('group-c'))));
 await page.evaluate(()=>testUI.toggleBulkSelection('chats',testUI.groupKey('group-c')));
 await page.locator('#bulk-toolbar button').nth(1).click();
 await page.locator('.modal:not(.hidden) [data-yes]').click();
 assert.deepEqual(await page.evaluate(()=>window.sent.filter(x=>x.type==='group_forget')), [{type:'group_forget',group:'group-c'}]);
 await page.evaluate(()=>testUI.toggleBulkSelection('messages','a'));
 await page.getByRole('button',{name:'Удалить у себя',exact:true}).last().click();
 // The custom confirmation is scoped to its captured conversation.
 await page.evaluate(()=>{testUI.state.current='bob'});
 const confirm=page.locator('.modal:not(.hidden)').last();
 await confirm.getByRole('button').last().click();
 assert.deepEqual(await page.evaluate(()=>window.sent.filter(x=>x.type==='delete_message')), [{type:'delete_message',conversation:'thread-a',id:'a',for_both:false}]);
 await page.evaluate(()=>{testUI.state.current='alice'});
 // UI preference must update its pressed state immediately.
 await page.evaluate(()=>document.querySelector('#divider-segment [data-dividers="none"]').click());
 assert.equal(await page.locator('#divider-segment [data-dividers="none"]').getAttribute('class'),'active');
 await page.evaluate(async()=>{
  const c=document.createElement('canvas');c.width=600;c.height=400;const ctx=c.getContext('2d');ctx.fillStyle='#fff';ctx.fillRect(0,0,600,400);
  const blob=await new Promise(resolve=>c.toBlob(resolve));await testUI.openEditor(blob,'message','alice');
 });
 const crop=await page.evaluate(()=>JSON.stringify(testUI.editor.crop));
 const before=await page.evaluate(()=>testUI.renderPhoto(700000,.9).data);
 await page.locator('#editor-zoom').fill('2');
 assert.equal(await page.evaluate(()=>JSON.stringify(testUI.editor.crop)),crop);
 assert.equal(await page.evaluate(()=>testUI.renderPhoto(700000,.9).data),before);
 await page.locator('#editor-draw').click();
 const box=await page.locator('#editor-canvas').boundingBox();
 await page.mouse.move(box.x+box.width*.4,box.y+box.height*.45);await page.mouse.down();await page.mouse.move(box.x+box.width*.6,box.y+box.height*.55,{steps:6});await page.mouse.up();
 assert.equal(await page.evaluate(()=>testUI.editor.strokes.length),1);
 assert.notEqual(await page.evaluate(()=>testUI.renderPhoto(700000,.9).data),before);
 await page.locator('#editor-undo-stroke').click();
 assert.equal(await page.evaluate(()=>testUI.renderPhoto(700000,.9).data),before);
 await page.evaluate(()=>{testUI.state.current='bob'});
 await page.locator('#editor-send').click();
 assert.equal(await page.evaluate(()=>window.sent.filter(x=>x.type==='send').at(-1).recipient_device),'alice');
 // Closing during an active drawing gesture must release window handlers.
 await page.evaluate(async()=>{
  const c=document.createElement('canvas');c.width=300;c.height=200;
  const blob=await new Promise(resolve=>c.toBlob(resolve));await testUI.openEditor(blob,'message','bob');
 });
 await page.locator('#editor-draw').click();
 const again=await page.locator('#editor-canvas').boundingBox();
 await page.mouse.move(again.x+40,again.y+40);await page.mouse.down();
 await page.keyboard.press('Escape');await page.mouse.move(again.x+60,again.y+60);await page.mouse.up();
 assert.deepEqual(errors,[]);
 console.log('PASS: selection/copy, chat/group pins and deletion, delete confirmation, dividers, zoom invariance, drawing/undo, captured photo recipient; no page errors');
 } finally { await browser.close(); server.close(); }
})().catch(e=>{console.error(e);server.close();process.exitCode=1;});
