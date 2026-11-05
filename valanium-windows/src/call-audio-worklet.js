/* Live mono PCM only; no recording files or persistent buffers. */
class CallCapture extends AudioWorkletProcessor {
  constructor(){super();this.phase=0;this.sum=0;this.count=0;this.frame=new Int16Array(640);this.at=0;}
  process(inputs,outputs){
    const input=inputs[0]?.[0];if(input)for(const sample of input){
      this.sum+=sample;this.count++;this.phase+=16000;
      if(this.phase>=sampleRate){this.phase-=sampleRate;const value=Math.max(-1,Math.min(1,this.sum/this.count));this.sum=0;this.count=0;
        this.frame[this.at++]=Math.round(value*32767);
        if(this.at===640){this.port.postMessage(this.frame.buffer,[this.frame.buffer]);this.frame=new Int16Array(640);this.at=0;}
      }
    }
    for(const output of outputs)for(const channel of output)channel.fill(0);return true;
  }
}
class CallPlayback extends AudioWorkletProcessor {
  constructor(){super();this.queue=[];this.current=null;this.at=0;this.phase=0;this.last=0;this.waiting=true;
    this.port.onmessage=event=>{const pcm=new Int16Array(event.data);if(pcm.length!==640)return;this.queue.push(pcm);while(this.queue.length>8)this.queue.shift();};
  }
  next(){
    if(this.waiting&&this.queue.length<3)return this.last*=.98;
    this.waiting=false;
    if(!this.current||this.at>=this.current.length){this.current=this.queue.shift();this.at=0;if(!this.current){this.waiting=true;return this.last*=.98;}}
    return this.last=this.current[this.at++]/32768;
  }
  process(inputs,outputs){for(const output of outputs)for(const channel of output)for(let i=0;i<channel.length;i++){this.phase+=16000;while(this.phase>=sampleRate){this.phase-=sampleRate;this.next();}channel[i]=this.last;}return true;}
}
registerProcessor('valanium-call-capture',CallCapture);registerProcessor('valanium-call-playback',CallPlayback);
