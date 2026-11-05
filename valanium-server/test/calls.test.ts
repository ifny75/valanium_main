import { test } from "node:test";
import assert from "node:assert/strict";
import { CallRelay,CALL_MEDIA,CALL_READY } from "../src/ws/calls.ts";
import type { Socket } from "../src/ws/registry.ts";
class Peer implements Socket {
 frames:Uint8Array[]=[];ended=false;buffer=0;
 send(bytes:Uint8Array){this.frames.push(bytes.slice());return 1;}
 getBufferedAmount(){return this.buffer;}
 end(){this.ended=true;}close(){this.end();}
}
const join=(room:string,role:number)=>new TextEncoder().encode(JSON.stringify({v:1,room,role}));
test("relay pairs only two roles, forwards opaque bounded media and closes both peers",()=>{
 const r=new CallRelay();const a=new Peer(),b=new Peer(),third=new Peer();const room="ab".repeat(32);
 assert.ok(r.join(a,join(room,0),100));assert.ok(r.join(b,join(room,1),100));assert.equal(r.join(third,join(room,1),100),false);
 assert.equal(a.frames[0]![0],CALL_READY);const ciphertext=new Uint8Array(100).fill(17);r.media(a,ciphertext,110);
 assert.equal(b.frames.at(-1)![0],CALL_MEDIA);assert.deepEqual(b.frames.at(-1)!.slice(1),ciphertext);
 b.buffer=20_000;const before=b.frames.length;r.media(a,ciphertext,120);assert.equal(b.frames.length,before);
 a.end();r.leave(a);assert.ok(a.ended&&b.ended);assert.equal(r.rooms.size,0);assert.equal(r.members.size,0);
});
test("room capacity, expiry, duplicate joins and frame flood are bounded",()=>{
 const r=new CallRelay(1),a=new Peer(),b=new Peer();assert.equal(r.join(a,join("bad",0),1),false);
 assert.ok(r.join(a,join("11".repeat(32),0),1));assert.equal(r.join(b,join("22".repeat(32),0),1),false);
 r.sweep(90_002);assert.equal(r.rooms.size,0);
 const c=new Peer();assert.ok(r.join(c,join("33".repeat(32),0),100_000));for(let i=0;i<61;i++)r.media(c,new Uint8Array(100),100_000);
 assert.ok(c.ended);assert.equal(r.rooms.size,0);
});

test("native close callback never calls end on the already closed socket",()=>{
 class NativePeer extends Peer {override end(){if(this.ended)throw new Error("closed native socket");this.ended=true;}}
 const r=new CallRelay(),a=new NativePeer(),b=new NativePeer();r.join(a,join("66".repeat(32),0),1);r.join(b,join("66".repeat(32),1),1);
 a.end();assert.doesNotThrow(()=>r.leave(a));assert.ok(b.ended);assert.equal(r.members.size,0);assert.equal(r.rooms.size,0);
});
