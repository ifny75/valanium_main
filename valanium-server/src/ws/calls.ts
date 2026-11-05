/** Ephemeral two-party relay. No identity, plaintext audio, disk queue or logs. */
import type { Socket } from "./registry.ts";
import { OP } from "../proto/frames.ts";
export const CALL_JOIN=OP.CALL_JOIN, CALL_READY=OP.CALL_READY, CALL_MEDIA=OP.CALL_MEDIA;
type Member={room:string;role:0|1;tokens:number;at:number};
type Room={slots:[Socket|null,Socket|null];created:number;last:number};
export class CallRelay {
  readonly rooms=new Map<string,Room>();
  readonly members=new Map<Socket,Member>();
  readonly maxRooms:number;
  constructor(maxRooms=512) { this.maxRooms=maxRooms; }
  has(socket:Socket):boolean{return this.members.has(socket);}
  sweep(now:number):void {
    for(const [id,room] of this.rooms) if(now-room.last>(room.slots[0]&&room.slots[1]?30_000:90_000)||now-room.created>7_200_000) this.close(id);
  }
  close(id:string):void {
    const room=this.rooms.get(id);if(!room)return;this.rooms.delete(id);
    for(const socket of room.slots)if(socket)this.members.delete(socket);
    for(const socket of room.slots)if(socket){try{socket.end(1000,"call ended");}catch{/* A native socket can already be closed. The room is still removed. */}}
  }
  leave(socket:Socket):void {const member=this.members.get(socket);if(!member)return;this.members.delete(socket);const room=this.rooms.get(member.room);if(room)room.slots[member.role]=null;this.close(member.room);}
  join(socket:Socket,raw:Uint8Array,now:number):boolean {
    this.sweep(now);if(raw.length>256||this.members.has(socket))return false;
    let value:unknown;try{value=JSON.parse(new TextDecoder().decode(raw));}catch{return false;}
    if(!value||typeof value!=="object"||Array.isArray(value))return false;
    const v=value as Record<string,unknown>;
    if(Object.keys(v).sort().join(",")!=="role,room,v"||v.v!==1||typeof v.room!=="string"||!/^([a-f0-9]{64})$/.test(v.room)||(v.role!==0&&v.role!==1))return false;
    let room=this.rooms.get(v.room);
    if(!room){if(this.rooms.size>=this.maxRooms)return false;room={slots:[null,null],created:now,last:now};this.rooms.set(v.room,room);}
    if(room.slots[v.role])return false;
    room.slots[v.role]=socket;room.last=now;this.members.set(socket,{room:v.room,role:v.role,tokens:60,at:now});
    if(room.slots[0]&&room.slots[1])for(const peer of room.slots)peer!.send(new Uint8Array([CALL_READY]),true);
    return true;
  }
  media(socket:Socket,wire:Uint8Array,now:number):void {
    const member=this.members.get(socket);if(!member){socket.end(1008,"not joined");return;}
    const room=this.rooms.get(member.room);if(!room)return;
    member.tokens=Math.min(60,member.tokens+Math.max(0,now-member.at)*0.04);member.at=now;
    if(wire.length<51||wire.length>1500||member.tokens<1||now-room.created>7_200_000){this.close(member.room);return;}
    member.tokens--;room.last=now;const peer=room.slots[member.role===0?1:0];
    // Live media is dropped under pressure; never accumulate delayed audio.
    try{if(peer&&peer.getBufferedAmount()<16_384){const frame=new Uint8Array(wire.length+1);frame[0]=CALL_MEDIA;frame.set(wire,1);if(peer.send(frame,true)===2)this.close(member.room);}}catch{this.close(member.room);}
  }
}
export const callRelay=new CallRelay();
const sweep=setInterval(()=>callRelay.sweep(Date.now()),5_000);sweep.unref();
