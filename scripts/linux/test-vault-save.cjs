const test = require('node:test');
const assert = require('node:assert/strict');
const vm = require('node:vm');
const fs = require('node:fs');
const path = require('node:path');
const source = fs.readFileSync(path.join(__dirname,'../../src-tauri/src/linux_vault_save.js'),'utf8');
function fixture() {
  class Input {
    constructor(type,name,value) { Object.assign(this,{type,name,value,autocomplete:'',isConnected:true,disabled:false,readOnly:false}); }
    getClientRects(){return [1];} hasAttribute(){return false;} closest(){return this;}
  }
  const href = 'https://example.test/login', user = new Input('text','username','synthetic-user'), pass = new Input('password','password','synthetic-secret');
  const button = new Input('submit','submit','');
  const form = {isConnected:true,method:'post',action:'https://example.test/session',target:'',elements:[user,pass,button]};
  for(const e of form.elements) e.form=form;
  const events = new Map(), messages = [], timers = new Map(); let sequence=0, now=1000;
  const document={addEventListener:(t,f)=>events.set(t,f),removeEventListener:(t)=>events.delete(t)};
  const window={webkit:{messageHandlers:{channel:{postMessage:s=>messages.push(JSON.parse(s))}}},addEventListener:(t,f)=>events.set(t,f),removeEventListener:t=>events.delete(t)};window.top=window;
  const location=new URL(href);
  const context=vm.createContext({window,document,location,URL,Date:{now:()=>now},isSecureContext:true,HTMLInputElement:Input,getComputedStyle:()=>({visibility:'visible'}),setTimeout:f=>{timers.set(++sequence,f);return sequence;},clearTimeout:i=>timers.delete(i)});
  const install=vm.runInContext('('+source+')',context);assert.equal(install('channel','ticket',href),true);
  const fire=(t,target,trusted=true,extra={})=>events.get(t)?.({target,isTrusted:trusted,...extra});
  const trigger=()=>{fire('input',pass);fire('click',button);fire('submit',form);};
  return {user,pass,button,form,location,window,context,events,messages,timers,fire,trigger,Input,advance:ms=>{now+=ms;},take:(ticket='ticket',id=1)=>context.__bushidoSaveTake?.(ticket,id),install,href};
}
test('trusted submit sends metadata only; one authorized read',()=>{const f=fixture();f.trigger();assert.deepEqual(f.messages,[{ticket:'ticket',href:f.href,id:1,username:'synthetic-user'}]);assert.equal(JSON.parse(f.take()).password,'synthetic-secret');assert.equal(f.take(),null);});
test('synthetic input, click or submit cannot arm',()=>{for(const stage of ['input','click','submit']){const f=fixture();for(const [t,e] of [['input',f.pass],['click',f.button],['submit',f.form]])f.fire(t,e,t!==stage);assert.equal(f.messages.length,0);}});
test('no trusted gesture or typed password: no candidate',()=>{for(const t of ['input','click']){const f=fixture();f.fire(t,t==='input'?f.pass:f.button);f.fire('submit',f.form);assert.equal(f.messages.length,0);}});
test('cross-origin action, GET, target, multiple passwords, override rejected',()=>{for(const change of [f=>f.form.action='https://other.test/',f=>f.form.method='get',f=>f.form.target='_blank',f=>f.form.elements.push(new f.Input('password','x','x')),f=>f.button.hasAttribute=a=>a==='formaction',f=>f.pass.autocomplete='new-password']){const f=fixture();change(f);f.trigger();assert.equal(f.messages.length,0);}});
test('ticket mismatch consumes candidate without delivering',()=>{const f=fixture();f.trigger();assert.equal(f.take('wrong'),null);assert.equal(f.take(),null);});
test('form action, password, username, document or fields changed: reject',()=>{for(const change of [f=>f.form.action='https://example.test/changed',f=>f.pass.value='changed',f=>f.user.value='changed',f=>f.location.pathname='/next',f=>f.form.elements[1]=new f.Input('password','password','synthetic-secret'),f=>f.form.isConnected=false]){const f=fixture();f.trigger();change(f);assert.equal(f.take(),null);}});
test('expiry and pagehide discard candidate',()=>{let f=fixture();f.trigger();[...f.timers.values()][0]();assert.equal(f.take(),null);f=fixture();f.trigger();f.fire('pagehide',f.window);assert.equal(f.take(),undefined);assert.equal(f.events.size,0);});
test('duplicate submit and new input invalidate or suppress pending',()=>{const f=fixture();f.trigger();f.trigger();assert.equal(f.messages.length,2);assert.equal(f.take('ticket',1),null);const g=fixture();g.trigger();g.fire('click',g.button);g.fire('submit',g.form);assert.equal(g.messages.length,1);g.fire('input',g.pass);assert.equal(g.take(),null);});
test('elapsed deadline rejects even if timer not scheduled',()=>{const f=fixture();f.trigger();f.advance(20000);assert.equal(f.take(),null);});
test('explicit discard only clears matching ticket and candidate',()=>{const f=fixture();f.trigger();f.context.__bushidoSaveDiscard('wrong',1);f.context.__bushidoSaveDiscard('ticket',9);assert.equal(JSON.parse(f.take()).password,'synthetic-secret');const g=fixture();g.trigger();g.context.__bushidoSaveDiscard('ticket',1);assert.equal(g.take(),null);g.fire('click',g.button);g.fire('submit',g.form);assert.equal(g.messages.length,1);});
