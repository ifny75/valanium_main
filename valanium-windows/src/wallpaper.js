// Appearance photos stay on this device, separately from account preferences.
export async function wallpaperPhoto(value) {
  const db = await new Promise((resolve,reject)=>{
    const request=indexedDB.open("valanium.appearance",1);
    request.onupgradeneeded=()=>request.result.createObjectStore("images");
    request.onsuccess=()=>resolve(request.result);request.onerror=()=>reject(request.error);
  });
  try {
    return await new Promise((resolve,reject)=>{
      const tx=db.transaction("images",value===undefined?"readonly":"readwrite");
      const store=tx.objectStore("images");
      const request=value===undefined?store.get("chat-wallpaper"):
        value===null?store.delete("chat-wallpaper"):store.put(value,"chat-wallpaper");
      tx.oncomplete=()=>resolve(request.result);tx.onerror=()=>reject(tx.error);tx.onabort=()=>reject(tx.error);
    });
  } finally {db.close();}
}

export async function prepareWallpaper(file) {
  const image=await createImageBitmap(file);
  try {
    const scale=Math.min(1,2048/Math.max(image.width,image.height));
    const canvas=document.createElement("canvas");
    canvas.width=Math.max(1,Math.round(image.width*scale));canvas.height=Math.max(1,Math.round(image.height*scale));
    const context=canvas.getContext("2d",{alpha:false});context.drawImage(image,0,0,canvas.width,canvas.height);
    return await new Promise((resolve,reject)=>canvas.toBlob(blob=>blob?resolve(blob):reject(Error("Image encoding failed")),"image/jpeg",.85));
  } finally {image.close();}
}
