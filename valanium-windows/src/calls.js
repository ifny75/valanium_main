export function createCalls({send,tr,nameOf,paintAvatar=()=>{},notify,ringEnabled=()=>true}){
 let current=null,context=null,stream=null,capture=null,playback=null,source=null,muted=false,panel=null,starting=false;
 let epoch=0,ringing=null,notified=false;
 // Live voice levels (0..1) for the bars; numbers only, the audio itself is never kept.
 let peerLevel=0,peerAt=0,ownLevel=0,ownAt=0,frame=0,ticker=0,timedId='',activeSince=0,compact=false;
 const heights=[.2,.2,.2,.2,.2];const SHAPE=[.55,.85,1,.8,.5];
 const calm=()=>matchMedia('(prefers-reduced-motion: reduce)').matches;
 function level(bytes){let sum=0,n=0;for(let i=0;i+1<bytes.length;i+=4){let v=bytes[i]|(bytes[i+1]<<8);if(v>32767)v-=65536;sum+=v*v;n++;}return Math.min(1,Math.sqrt(Math.sqrt(sum/Math.max(1,n))/3500));}
 function elapsed(){const s=Math.max(0,Math.floor((Date.now()-activeSince)/1000));const two=n=>String(n).padStart(2,'0');return s>=3600?`${Math.floor(s/3600)}:${two(Math.floor(s/60)%60)}:${two(s%60)}`:`${two(Math.floor(s/60))}:${two(s%60)}`;}
 function statusText(){return current?.status==='active'&&current.call_id===timedId?elapsed():tr(labels[current?.status]||current?.status||'');}
 function animate(){
  frame=0;if(!panel)return;
  const now=performance.now(),peer=now-peerAt>250?0:peerLevel,own=now-ownAt>250||muted?0:ownLevel,t=now/1000,still=calm();
  const bars=panel.querySelectorAll('.call-bars i');
  bars.forEach((bar,i)=>{const wobble=still?0:.22*Math.sin(t*(7+i*1.7)+i*1.3)*peer;const target=.2+.8*Math.min(1,Math.max(0,peer*SHAPE[i]*1.25+wobble));heights[i]+=(target-heights[i])*(still?1:.3);bar.style.transform=`scaleY(${heights[i].toFixed(3)})`;});
  panel.style.setProperty('--voice',peer.toFixed(3));panel.style.setProperty('--own',own.toFixed(3));
  if(['ringing','connecting','active'].includes(current?.status))frame=requestAnimationFrame(animate);
 }
 function startTicker(){clearInterval(ticker);ticker=setInterval(()=>{const node=panel?.querySelector('[data-call-state]');if(node&&!node.dataset.error)node.textContent=statusText();},1000);}
 function stopRing(){const active=ringing;ringing=null;void active?.close().catch(()=>{});}
 function ring(){
  if(ringing||!ringEnabled())return;
  try{
   ringing=new AudioContext();const volume=ringing.createGain();volume.gain.value=0;volume.connect(ringing.destination);
   for(const hz of [440,480]){const tone=ringing.createOscillator();tone.frequency.value=hz;tone.connect(volume);tone.start();}
   const base=ringing.currentTime;for(let i=0;i<30;i++){const at=base+i*3;volume.gain.setValueAtTime(0,at);volume.gain.linearRampToValueAtTime(.08,at+.02);volume.gain.setValueAtTime(.08,at+.75);volume.gain.linearRampToValueAtTime(0,at+.8);}
   void ringing.resume().catch(()=>{});
   setTimeout(()=>{if(current?.status==='incoming'&&ringing?.state!=='running'&&!notified){notified=true;notify({title:'Valanium',text:tr('Входящий звонок'),device:current.peer_device});}},300);
  }catch{}
 }

 const labels={incoming:'Входящий звонок',ringing:'Ожидание ответа…',connecting:'Соединяем аудиоканал…',active:'Звонок · сквозное шифрование',ended:'Звонок завершён',failed:'Соединение звонка потеряно',declined:'Звонок отклонён',busy:'У собеседника другой звонок',timeout:'Нет ответа на звонок'};
 async function permission(){
  const version=epoch;
  if(!context){context=new AudioContext();await context.audioWorklet.addModule(new URL('./call-audio-worklet.js',import.meta.url));}
  if(version!==epoch)throw new Error("cancelled");
  await context.resume();
  if(!stream){const acquired=await navigator.mediaDevices.getUserMedia({audio:{channelCount:1,echoCancellation:true,noiseSuppression:true,autoGainControl:true},video:false});if(version!==epoch){acquired.getTracks().forEach(track=>track.stop());throw new Error("cancelled");}stream=acquired;stream.getAudioTracks().forEach(track=>{track.enabled=false;});}
 }
 function release(){epoch++;starting=false;source?.disconnect();capture?.disconnect();playback?.disconnect();source=capture=playback=null;stream?.getTracks().forEach(track=>track.stop());stream=null;void context?.close().catch(()=>{});context=null;}
 function close(){stopRing();notified=false;release();current=null;clearTimeout(autoClose);clearInterval(ticker);cancelAnimationFrame(frame);frame=0;panel?.remove();panel=null;muted=false;compact=false;}
 async function audio(){
  if(capture||starting)return;starting=true;const id=current?.call_id;
  try{
   await permission();if(!current||current.call_id!==id||!["connecting","active"].includes(current.status)){release();return;}
   stream.getAudioTracks().forEach(track=>{track.enabled=!muted;});
   playback=new AudioWorkletNode(context,'valanium-call-playback',{outputChannelCount:[1]});playback.connect(context.destination);
   capture=new AudioWorkletNode(context,'valanium-call-capture',{outputChannelCount:[1]});source=context.createMediaStreamSource(stream);source.connect(capture);capture.connect(context.destination);
   capture.port.onmessage=event=>{if(!current||current.call_id!==id||muted)return;const bytes=new Uint8Array(event.data);ownLevel=level(bytes);ownAt=performance.now();const data=Array.from(bytes,b=>b.toString(16).padStart(2,'0')).join('');void send({type:'call_audio',call_id:id,data,captured_at:Date.now()});};
  }catch{if(current){void send({type:'call_end',call_id:current.call_id});showError('Не удалось включить микрофон. Проверьте разрешение приложения.');}release();}
  finally{starting=false;}
 }
 function showError(message){const node=panel?.querySelector('[data-call-state]');if(node){node.dataset.error='1';node.textContent=tr(message);}}
 const icons={
  accept:'M6.6 10.8c1.4 2.8 3.8 5.2 6.6 6.6l2.2-2.2c.3-.3.7-.4 1.1-.3 1.1.4 2.3.6 3.5.6.6 0 1 .4 1 1V20c0 .6-.4 1-1 1C10.6 21 3 13.4 3 4c0-.6.4-1 1-1h3.5c.6 0 1 .4 1 1 0 1.2.2 2.4.6 3.5.1.4 0 .8-.3 1.1z',
  end:'M12 9c-1.6 0-3.15.25-4.6.72v3.1c0 .39-.23.74-.56.9-.98.49-1.87 1.12-2.66 1.85-.18.18-.43.28-.7.28-.28 0-.53-.11-.71-.29L.29 13.08a.996.996 0 0 1 0-1.41C3.34 8.78 7.46 7 12 7s8.66 1.78 11.71 4.67c.39.39.39 1.02 0 1.41l-2.48 2.48c-.18.18-.43.29-.71.29-.27 0-.52-.11-.7-.28-.79-.74-1.69-1.36-2.67-1.85-.33-.16-.56-.5-.56-.9v-3.1C15.15 9.25 13.6 9 12 9z',
  mic:'M12 14a3 3 0 0 0 3-3V5a3 3 0 0 0-6 0v6a3 3 0 0 0 3 3zm5.3-3c0 3-2.5 5.1-5.3 5.1S6.7 14 6.7 11H5c0 3.4 2.7 6.2 6 6.7V21h2v-3.3c3.3-.5 6-3.3 6-6.7h-1.7z',
  collapse:'M7.4 15.4 12 10.8l4.6 4.6L18 14l-6-6-6 6z',
  expand:'M7.4 8.6 12 13.2l4.6-4.6L18 10l-6 6-6-6z',
  micOff:'M19 11h-1.7c0 .74-.16 1.43-.43 2.05l1.23 1.23c.56-.98.9-2.09.9-3.28zm-4.02.17c0-.06.02-.11.02-.17V5c0-1.66-1.34-3-3-3S9 3.34 9 5v.18l5.98 5.99zM4.27 3 3 4.27l6.01 6.01V11c0 1.66 1.33 3 2.99 3 .22 0 .44-.03.65-.08l1.66 1.66c-.71.33-1.5.52-2.31.52-2.76 0-5.3-2.1-5.3-5.1H5c0 3.41 2.72 6.23 6 6.72V21h2v-3.28c.91-.13 1.77-.45 2.54-.9L19.73 21 21 19.73 4.27 3z',
 };
 const FINAL=['ended','failed','declined','busy','timeout'];
 let autoClose=0;
 function control(kind,icon,label,run){
  const b=document.createElement('button');b.type='button';b.className=`call-control ${kind}`;b.title=tr(label);b.setAttribute('aria-label',tr(label));
  const svg=document.createElementNS('http://www.w3.org/2000/svg','svg');svg.setAttribute('viewBox','0 0 24 24');svg.setAttribute('aria-hidden','true');
  const path=document.createElementNS('http://www.w3.org/2000/svg','path');path.setAttribute('d',icons[icon]);svg.append(path);b.append(svg);b.onclick=run;return b;
 }
 function paint(){
  if(!current)return;
  clearTimeout(autoClose);
  if(!panel){panel=document.createElement('section');panel.id='call-panel';panel.className='call-panel';panel.setAttribute('role','dialog');panel.setAttribute('aria-label',tr('Аудиозвонок'));document.body.append(panel);}
  const live=['ringing','connecting','active'].includes(current.status);
  panel.dataset.status=current.status;panel.classList.toggle('compact',compact&&live);panel.replaceChildren();
  const halo=document.createElement('div');halo.className='call-halo';
  const avatar=document.createElement('div');avatar.className='call-avatar';paintAvatar(avatar,current.peer_device);halo.append(avatar);
  const copy=document.createElement('div');copy.className='call-copy';
  const title=document.createElement('b');title.textContent=nameOf(current.peer_device);
  const state=document.createElement('span');state.dataset.callState='';state.textContent=statusText();
  copy.append(title,state);
  const bars=document.createElement('div');bars.className='call-bars';bars.setAttribute('aria-hidden','true');for(let i=0;i<5;i++)bars.append(document.createElement('i'));
  const actions=document.createElement('div');actions.className='call-actions';
  if(current.status==='incoming'){
   actions.append(control('decline','end','Отклонить',()=>{void send({type:'call_reject',call_id:current.call_id});close();}));
   actions.append(control('accept','accept','Принять',async()=>{const id=current.call_id;try{await permission();if(current?.call_id===id)void send({type:'call_accept',call_id:id});}catch{showError('Не удалось включить микрофон. Проверьте разрешение приложения.');}}));
  }else if(['ringing','connecting','active'].includes(current.status)){
   actions.append(control(muted?'toggle on':'toggle',muted?'micOff':'mic',muted?'Включить микрофон':'Выключить микрофон',()=>{muted=!muted;stream?.getAudioTracks().forEach(track=>{track.enabled=!muted;});void send({type:'call_mute',call_id:current.call_id,muted});paint();}));
   actions.append(control('decline','end','Завершить',()=>{void send({type:'call_end',call_id:current.call_id});close();}));
  }else{
   actions.append(control('toggle','end','Закрыть',close));
   // The other side finished: show the outcome briefly, then remove the banner.
   if(current.status!=='failed'){const shown=current;autoClose=setTimeout(()=>{if(current===shown)close();},2600);}
  }
  const privacy=document.createElement('small');privacy.textContent=tr('Звук через relay без Tor. Сервер видит IP участников, но не расшифровывает звук.');
  const head=document.createElement('div');head.className='call-head';head.append(halo,copy);
  if(live){
   head.append(bars);
   const fold=control('fold',compact?'expand':'collapse',compact?'Развернуть звонок':'Свернуть звонок',()=>{compact=!compact;paint();});
   head.append(fold);
   // In the compact pill the whole strip brings the full banner back.
   if(compact){head.tabIndex=0;head.setAttribute('role','button');head.setAttribute('aria-label',tr('Развернуть звонок'));head.onclick=event=>{if(event.target.closest('button'))return;compact=false;paint();};}
  }
  panel.append(head,actions,privacy);
  if(live&&!frame)frame=requestAnimationFrame(animate);
  if(live)startTicker();else clearInterval(ticker);
 }
 async function start(peer){
  if(current&&['incoming','ringing','connecting','active'].includes(current.status)){paint();return;}
  close();current={peer_device:peer,call_id:'',status:'connecting'};paint();
  const request=current;
  try{await permission();if(current===request)await send({type:'call_start',recipient_device:peer});}catch{if(current!==request)return;release();current.status='failed';paint();showError('Не удалось включить микрофон. Проверьте разрешение приложения.');}
 }
 function event(event){
  if(event.type==='call_audio'){
   if(!playback||event.call_id!==current?.call_id||!/^[a-f0-9]{2560}$/.test(event.data))return;
   const bytes=new Uint8Array(1280);for(let i=0;i<bytes.length;i++)bytes[i]=parseInt(event.data.slice(i*2,i*2+2),16);peerLevel=level(bytes);peerAt=performance.now();playback.port.postMessage(bytes.buffer,[bytes.buffer]);return;
  }
  if(event.status==='idle'){close();return;}if(!(event.status in labels))return;
  // Closed locally already: the core's final status must not bring the banner back.
  if(!current&&FINAL.includes(event.status))return;
  if(current?.call_id&&event.call_id!==current.call_id&&event.status!=='incoming')return;
  if(event.status==='incoming'&&current?.call_id!==event.call_id){notified=false;stopRing();}
  if(event.status!=="incoming")stopRing();
  if(event.status==='active'&&event.call_id!==timedId){timedId=event.call_id;activeSince=Date.now();}
  if(event.status==='incoming')compact=false;
  current=event;paint();
  if(event.status==='incoming'){ring();if((document.hidden||!document.hasFocus())&&!notified){notified=true;notify({title:'Valanium',text:tr('Входящий звонок'),device:event.peer_device});}}
  if(['connecting','active'].includes(event.status))void audio();
  if(FINAL.includes(event.status))release();
 }
 function failure(){stopRing();if(current){current.status='failed';paint();release();}}
 window.addEventListener('beforeunload',()=>{if(current?.call_id)void send({type:'call_end',call_id:current.call_id});stopRing();release();});
 return{start,event,failure,close};
}
