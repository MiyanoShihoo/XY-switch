import assert from 'node:assert/strict';
import { writeFile, mkdir } from 'node:fs/promises';
import { dirname } from 'node:path';
import { navigateFreshDocument } from './document-navigation.mjs';

export function isLocalRendererRequest(value, base) {
  try {
    const renderer = new URL(base), request = new URL(value);
    if (!['http:', 'https:'].includes(renderer.protocol) || !['127.0.0.1', 'localhost', '[::1]'].includes(renderer.hostname)) return false;
    if (['data:', 'blob:', 'about:'].includes(request.protocol)) return true;
    if (request.origin === renderer.origin) return true;
    return request.protocol === (renderer.protocol === 'https:' ? 'wss:' : 'ws:') && request.host === renderer.host;
  } catch { return false; }
}

export async function connect(endpoint = process.env.CDP_ENDPOINT ?? 'http://127.0.0.1:9231', { fresh = false } = {}) {
  const rendererBase = process.env.APP_URL ?? 'http://127.0.0.1:1431';
  assert(isLocalRendererRequest(rendererBase, rendererBase), 'QA renderer URL must be a local HTTP(S) origin');
  const url = new URL(endpoint);
  if (!['127.0.0.1','localhost','[::1]'].includes(url.hostname)) throw new Error('QA CDP must be local');
  const pages = await fetch(endpoint+'/json').then(r=>r.json());
  const page = fresh ? await fetch(endpoint+'/json/new?about:blank', { method: 'PUT' }).then(r=>r.json()) : pages.find(p=>p.type==='page' && (p.url==='about:blank' || p.url.startsWith(process.env.APP_URL ?? 'http://127.0.0.1:1431')));
  if (!page) throw new Error('No isolated QA target at '+endpoint);
  const socket = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((resolve,reject)=>{socket.addEventListener('open',resolve,{once:true});socket.addEventListener('error',reject,{once:true});});
  let sequence=0; const pending=new Map(), exceptions=[], networkRequests=[], webSocketUrls=[];
  socket.addEventListener('message',async event=>{
    const message=JSON.parse(typeof event.data==='string'?event.data:await event.data.text());
    if (message.method==='Runtime.exceptionThrown') exceptions.push(message.params.exceptionDetails);
    if (message.method==='Network.requestWillBeSent') networkRequests.push(message.params.request.url);
    if (message.method==='Network.webSocketCreated') webSocketUrls.push(message.params.url);
    if (!pending.has(message.id)) return;
    const {resolve,reject,timer}=pending.get(message.id);pending.delete(message.id);clearTimeout(timer);
    if(message.error)reject(new Error(message.error.message));else resolve(message.result);
  });
  function call(method,params={}) { return new Promise((resolve,reject)=>{const id=++sequence;const timer=setTimeout(()=>{pending.delete(id);reject(new Error('CDP timeout: '+method));},20000);pending.set(id,{resolve,reject,timer});socket.send(JSON.stringify({id,method,params}));}); }
  async function evaluate(expression) { const result=await call('Runtime.evaluate',{expression,awaitPromise:true,returnByValue:true});if(result.exceptionDetails)throw new Error(result.exceptionDetails.exception?.description??result.exceptionDetails.text);return result.result.value; }
  async function waitFor(expression, timeout=10000) { const start=Date.now();while(Date.now()-start<timeout){if(await evaluate(expression))return;await new Promise(r=>setTimeout(r,60));}throw new Error('Timed out: '+expression); }
  async function clickExpression(expression) {
    const point = await evaluate('(() => { const e = '+expression+'; if (!e) throw new Error("Missing click target"); if(e.disabled)throw new Error("Disabled click target");e.scrollIntoView({block:"center",inline:"nearest"});const r=e.getBoundingClientRect(); return {x:r.x+r.width/2,y:r.y+r.height/2}; })()');
    await call('Input.dispatchMouseEvent',{type:'mousePressed',button:'left',clickCount:1,...point});
    await call('Input.dispatchMouseEvent',{type:'mouseReleased',button:'left',clickCount:1,...point});
    await new Promise(r=>setTimeout(r,90));
  }
  const click=selector=>clickExpression('document.querySelector('+JSON.stringify(selector)+')');
  const button=(label,scope='document')=>clickExpression('[...'+scope+'.querySelectorAll("button")].find(e=>!e.closest("details:not([open])")&&!e.classList.contains("sr-only")&&e.getBoundingClientRect().width>1&&e.getBoundingClientRect().height>1&&getComputedStyle(e).visibility!=="hidden"&&(e.textContent.trim()==='+JSON.stringify(label)+'||e.getAttribute("aria-label")==='+JSON.stringify(label)+'))');
  async function fill(selector,value) { await evaluate('(() => { const e=document.querySelector('+JSON.stringify(selector)+'); if(!e)throw new Error("Missing field");const proto=e instanceof HTMLSelectElement?HTMLSelectElement.prototype:e instanceof HTMLTextAreaElement?HTMLTextAreaElement.prototype:HTMLInputElement.prototype;Object.getOwnPropertyDescriptor(proto,"value").set.call(e,'+JSON.stringify(value)+');e.dispatchEvent(new Event(e instanceof HTMLSelectElement?"change":"input",{bubbles:true})); })()');await new Promise(r=>setTimeout(r,70)); }
  const key=async(key,modifiers=0)=>{const windowsVirtualKeyCode=({Tab:9,Escape:27,Enter:13,ArrowRight:39,ArrowLeft:37,ArrowDown:40,ArrowUp:38,Home:36,End:35})[key];const text=key==='Enter'?'\r':undefined;await call('Input.dispatchKeyEvent',{type:'keyDown',key,code:key,modifiers,windowsVirtualKeyCode,text,unmodifiedText:text});await call('Input.dispatchKeyEvent',{type:'keyUp',key,code:key,modifiers,windowsVirtualKeyCode});await new Promise(r=>setTimeout(r,50));};
  async function screenshot(file){const image=await call('Page.captureScreenshot',{format:'png',fromSurface:true});await mkdir(dirname(file),{recursive:true});await writeFile(file,Buffer.from(image.data,'base64'));}
  const viewport=(width,height)=>call('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false});
  const navigate=url=>navigateFreshDocument({call,evaluate},url);
  const snapshot=()=>evaluate('({text:document.body.innerText,controls:[...document.querySelectorAll("button,input,select,textarea,summary")].filter(e=>e.getClientRects().length).map(e=>({tag:e.tagName,text:e.textContent.slice(0,120),label:e.getAttribute("aria-label"),id:e.id,value:e.value,disabled:e.disabled})),active:document.activeElement?.outerHTML.slice(0,300),width:innerWidth,scrollWidth:document.documentElement.scrollWidth})');
  const assertLocalNetwork = () => {
    const base = process.env.APP_URL ?? 'http://127.0.0.1:1431';
    const external = [...networkRequests, ...webSocketUrls].filter(value => !isLocalRendererRequest(value, base));
    assert.deepEqual([...new Set(external.map(value => { try { return new URL(value).origin; } catch { return '[invalid URL]'; } }))], [], 'QA renderer attempted requests outside its exact local origin');
  };
  await call('Page.enable');await call('Runtime.enable');await call('Network.enable');
  return {call,evaluate,waitFor,click,clickExpression,button,fill,key,screenshot,viewport,navigate,snapshot,exceptions,networkRequests,webSocketUrls,assertLocalNetwork,close:async()=>{socket.close();if(fresh)await fetch(endpoint+'/json/close/'+page.id);assertLocalNetwork();}};
}
