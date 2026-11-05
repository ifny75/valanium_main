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
 const browser=await chromium.launch({executablePath,args:['--no-sandbox','--use-fake-ui-for-media-stream','--use-fake-device-for-media-stream','--autoplay-policy=no-user-gesture-required']});
 try {
 const page=await browser.newPage({viewport:{width:1440,height:1000}});
 const errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.addInitScript(()=>{
  localStorage.setItem('valanium.onboarding.language','ru');
  window.sent=[]; window.nativeCalls=[];
  window.__TAURI__={core:{invoke:async(name,args)=>{window.nativeCalls.push({name,args});if(name==='submit')window.sent.push(JSON.parse(args.json));return name==='auto_unlock'?'locked':null;}},event:{listen:async()=>()=>{}}};
  navigator.clipboard.writeText=async text=>window.copied=text;
 });
 await page.route('**/main.js',route=>route.fulfill({contentType:'text/javascript',body:fs.readFileSync(path.join(root,'main.js'),'utf8')+'\nwindow.testUI={state,audioCalls,pairing,handlers,editor,bulkSelection,groupKey,showScreen,renderConversations,selectConversation,toggleBulkSelection,clearBulkSelection,openEditor,renderPhoto,buildMessage,encodeContent,preferences,applyPreferences,history};'}));
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
 // Receiver publishes a QR, but confirmation still requires an explicit click.
 await page.evaluate(()=>{testUI.pairing.stop();testUI.state.device='';testUI.showScreen('screen-entry');testUI.pairing.start(false);});
 await page.locator('#pairing-entry').click();
 assert.ok(await page.evaluate(()=>window.sent.some(x=>x.type==='pairing_listen')));
 await page.evaluate(()=>testUI.pairing.event({type:'pairing',session:'session-rx',state:'listening',qr:'valanium-pair:v2:test',qr_svg:'<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><rect width="100" height="100" fill="white"/></svg>'}));
 assert.equal(await page.locator('#pairing-modal img').isVisible(),true);
 await page.evaluate(()=>testUI.pairing.event({type:'pairing',session:'session-rx',state:'verify',code:'123456'}));
 assert.equal(await page.locator('#pairing-modal [data-pairing-confirm]').isVisible(),true);
 assert.equal(await page.evaluate(()=>window.sent.filter(x=>x.type==='pairing_confirm').length),0);
 await page.locator('#pairing-modal [data-pairing-confirm]').click();
 assert.deepEqual(await page.evaluate(()=>window.sent.filter(x=>x.type==='pairing_confirm').at(-1)),{type:'pairing_confirm',session:'session-rx'});
 await page.locator('#pairing-modal button').filter({hasText:'Закрыть'}).click();
 // A discovery hint never authenticates the peer or transmits account keys.
 await page.evaluate(()=>{
  testUI.state.device='owner';testUI.showScreen('screen-main');testUI.pairing.start(true);
  testUI.pairing.event({type:'pairing_found',session:'nearby',address:'192.168.1.3'});
  testUI.pairing.event({type:'pairing_found',session:'nearby',address:'192.168.1.3'});
 });
 assert.equal(await page.locator('#pairing-offer').count(),1);
 await page.locator('#pairing-offer button').first().click();
 await page.locator('#pairing-modal textarea').fill('valanium-pair:v2:test');
 await page.locator('#pairing-modal button').filter({hasText:'Подключить по коду'}).click();
 await page.evaluate(()=>{
  testUI.pairing.event({type:'pairing',session:'session-tx',state:'connecting'});
  testUI.pairing.event({type:'pairing',session:'session-tx',state:'verify',code:'123456'});
 });
 await page.locator('#pairing-modal [data-pairing-confirm]').click();
 assert.deepEqual(await page.evaluate(()=>window.nativeCalls.filter(x=>x.name==='pairing_confirm').at(-1).args),{session:'session-tx',password:null});
 await page.locator('#pairing-modal button').filter({hasText:'Закрыть'}).click();
 assert.equal(await page.locator('#pairing-modal').count(),0);
 assert.ok(await page.evaluate(()=>window.sent.some(x=>x.type==='pairing_cancel')));
 // Manual settings entry remains usable when automatic discovery is disabled.
 await page.evaluate(()=>{
  const toggle=document.querySelector('[data-pairing-discovery]');toggle.checked=false;toggle.dispatchEvent(new Event('change'));
  document.getElementById('pairing-settings').click();
 });
 assert.equal(await page.locator('#pairing-modal').count(),1);
 await page.locator('#pairing-modal button').filter({hasText:'Закрыть'}).click();
 assert.equal(await page.locator('#pairing-modal').count(),0);
 // Actual browser microphone + AudioWorklet capture, explicit accept and mute.
 await page.evaluate(()=>{
  window.microphoneRequests=0;window.callStreams=[];
  const original=navigator.mediaDevices.getUserMedia.bind(navigator.mediaDevices);
  navigator.mediaDevices.getUserMedia=async options=>{window.microphoneRequests++;const stream=await original(options);window.callStreams.push(stream);return stream;};
  void testUI.audioCalls.start('a'.repeat(64));
 });
 await page.waitForFunction(()=>window.sent.some(x=>x.type==='call_start'));
 await page.waitForTimeout(200);
 assert.equal(await page.evaluate(()=>window.sent.filter(x=>x.type==='call_audio').length),0);
 await page.evaluate(()=>testUI.handlers.call_status({call_id:'1'.repeat(32),peer_device:'a'.repeat(64),status:'connecting'}));
 await page.waitForFunction(()=>window.sent.filter(x=>x.type==='call_audio').length>=3);
 assert.equal(await page.evaluate(()=>window.sent.find(x=>x.type==='call_audio').data.length),2560);
 await page.locator('#call-panel button').filter({hasText:'Выключить микрофон'}).click();
 const mutedCount=await page.evaluate(()=>window.sent.filter(x=>x.type==='call_audio').length);await page.waitForTimeout(250);
 assert.equal(await page.evaluate(()=>window.sent.filter(x=>x.type==='call_audio').length),mutedCount);
 await page.locator('#call-panel button').filter({hasText:'Завершить'}).click();
 assert.equal(await page.evaluate(()=>window.callStreams.every(stream=>stream.getTracks().every(track=>track.readyState==='ended'))),true);
 const micRequests=await page.evaluate(()=>window.microphoneRequests);
 await page.evaluate(()=>testUI.handlers.call_status({call_id:'2'.repeat(32),peer_device:'a'.repeat(64),status:'incoming'}));
 assert.equal(await page.evaluate(()=>window.microphoneRequests),micRequests);
 await page.locator('#call-panel button').filter({hasText:'Принять',exact:true}).click();
 await page.waitForFunction(()=>window.sent.some(x=>x.type==='call_accept'&&x.call_id==='2'.repeat(32)));
 await page.evaluate(()=>testUI.handlers.call_status({call_id:'2'.repeat(32),peer_device:'a'.repeat(64),status:'ended'}));
 assert.equal(await page.evaluate(()=>window.callStreams.every(stream=>stream.getTracks().every(track=>track.readyState==='ended'))),true);
 await page.evaluate(()=>{
  testUI.handlers.group({group:'archived',kind:'group',title:'History',owner:'owner',members:[],history_only:true});
  testUI.selectConversation(testUI.groupKey('archived'));
 });
 assert.equal(await page.locator('#composer').isDisabled(),true);
 assert.match(await page.locator('#composer').getAttribute('placeholder'),/История группы/);
 assert.deepEqual(errors,[]);
 console.log('PASS: selection/copy, chat/group pins and deletion, delete confirmation, dividers, zoom invariance, drawing/undo, captured photo recipient, QR pairing UI, browser audio capture/mute/consent/cleanup; no page errors');
 } finally { await browser.close(); server.close(); }
})().catch(e=>{console.error(e);server.close();process.exitCode=1;});
