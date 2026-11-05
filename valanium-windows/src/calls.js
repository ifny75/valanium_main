export function createCalls({send,tr,nameOf,notify,ringEnabled=()=>true}){
 let current=null,context=null,stream=null,capture=null,playback=null,source=null,muted=false,panel=null,starting=false;
 let epoch=0,ringing=null,notified=false;
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
 function close(){stopRing();notified=false;release();current=null;panel?.remove();panel=null;muted=false;}
 async function audio(){
  if(capture||starting)return;starting=true;const id=current?.call_id;
  try{
   await permission();if(!current||current.call_id!==id||!["connecting","active"].includes(current.status)){release();return;}
   stream.getAudioTracks().forEach(track=>{track.enabled=!muted;});
   playback=new AudioWorkletNode(context,'valanium-call-playback',{outputChannelCount:[1]});playback.connect(context.destination);
   capture=new AudioWorkletNode(context,'valanium-call-capture',{outputChannelCount:[1]});source=context.createMediaStreamSource(stream);source.connect(capture);capture.connect(context.destination);
   capture.port.onmessage=event=>{if(!current||current.call_id!==id||muted)return;const bytes=new Uint8Array(event.data);const data=Array.from(bytes,b=>b.toString(16).padStart(2,'0')).join('');void send({type:'call_audio',call_id:id,data,captured_at:Date.now()});};
  }catch{if(current){void send({type:'call_end',call_id:current.call_id});showError('Не удалось включить микрофон. Проверьте разрешение приложения.');}release();}
  finally{starting=false;}
 }
 function showError(message){if(panel)panel.querySelector('[data-call-state]').textContent=tr(message);}
 function paint(){
  if(!current)return;
  if(!panel){panel=document.createElement('section');panel.id='call-panel';panel.className='call-panel';panel.setAttribute('role','dialog');panel.setAttribute('aria-label',tr('Аудиозвонок'));document.body.append(panel);}
  panel.replaceChildren();const title=document.createElement('h2');title.textContent=nameOf(current.peer_device);const state=document.createElement('p');state.dataset.callState='';state.setAttribute('aria-live','polite');state.textContent=tr(labels[current.status]||current.status);
  const privacy=document.createElement('small');privacy.textContent=tr('Звук через relay без Tor. Сервер видит IP участников, но не расшифровывает звук.');panel.append(title,state,privacy);
  const button=(text,run)=>{const b=document.createElement('button');b.type='button';b.className='ghost-button';b.textContent=tr(text);b.onclick=run;panel.append(b);};
  if(current.status==='incoming'){
   button('Принять',async()=>{const id=current.call_id;try{await permission();if(current?.call_id===id)void send({type:'call_accept',call_id:id});}catch{showError('Не удалось включить микрофон. Проверьте разрешение приложения.');}});
   button('Отклонить',()=>{void send({type:'call_reject',call_id:current.call_id});close();});
  }else if(['ringing','connecting','active'].includes(current.status)){
   button(muted?'Включить микрофон':'Выключить микрофон',()=>{muted=!muted;stream?.getAudioTracks().forEach(track=>{track.enabled=!muted;});void send({type:'call_mute',call_id:current.call_id,muted});paint();});
   button('Завершить',()=>{void send({type:'call_end',call_id:current.call_id});close();});
  }else button('Закрыть',close);
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
   const bytes=new Uint8Array(1280);for(let i=0;i<bytes.length;i++)bytes[i]=parseInt(event.data.slice(i*2,i*2+2),16);playback.port.postMessage(bytes.buffer,[bytes.buffer]);return;
  }
  if(event.status==='idle'){close();return;}if(!(event.status in labels))return;
  if(current?.call_id&&event.call_id!==current.call_id&&event.status!=='incoming')return;
  if(event.status==='incoming'&&current?.call_id!==event.call_id){notified=false;stopRing();}
  if(event.status!=="incoming")stopRing();
  current=event;paint();
  if(event.status==='incoming'){ring();if((document.hidden||!document.hasFocus())&&!notified){notified=true;notify({title:'Valanium',text:tr('Входящий звонок'),device:event.peer_device});}}
  if(['connecting','active'].includes(event.status))void audio();
  if(['ended','failed','declined','busy','timeout'].includes(event.status))release();
 }
 function failure(){stopRing();if(current){current.status='failed';paint();release();}}
 window.addEventListener('beforeunload',()=>{if(current?.call_id)void send({type:'call_end',call_id:current.call_id});stopRing();release();});
 return{start,event,failure,close};
}
