/** Convert an older own avatar to the compact profile format, once fetched. */
export async function avatarThumbnail(mime,base64){
  if(!mime?.startsWith("image/")||!base64)return null;
  const bytes=Uint8Array.from(atob(base64),c=>c.charCodeAt(0));
  const image=await createImageBitmap(new Blob([bytes],{type:mime}));
  try{
    if(Math.max(image.width,image.height)<=384)return null;
    const scale=384/Math.max(image.width,image.height),canvas=document.createElement("canvas");
    canvas.width=Math.max(1,Math.round(image.width*scale));canvas.height=Math.max(1,Math.round(image.height*scale));
    canvas.getContext("2d",{alpha:false}).drawImage(image,0,0,canvas.width,canvas.height);
    return canvas.toDataURL("image/jpeg",.8).split(",")[1];
  }finally{image.close();}
}
