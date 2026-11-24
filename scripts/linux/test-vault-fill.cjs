const test = require('node:test');
const assert = require('node:assert/strict');
const vm = require('node:vm');
const fs = require('node:fs');
const path = require('node:path');
const source = fs.readFileSync(path.join(__dirname, '../../src-tauri/src/linux_vault_fill.js'), 'utf8');
function fixture() {
  class Input {
    constructor(type, name) { Object.assign(this, {type, name, autocomplete:'', isConnected:true, disabled:false, readOnly:false, visible:true, events:[], _value:''}); }
    get value() { return this._value; }
    set value(v) { this._value = v; }
    getClientRects() { return this.visible ? [1] : []; }
    dispatchEvent(e) { this.events.push(e.type); }
  }
  const user = new Input('text','username'), pass = new Input('password','password');
  const origin = 'https://example.test', href = origin + '/login';
  const form = {isConnected:true, method:'post', action:origin+'/session', target:'', elements:[user,pass], querySelector(){return this.override || null;}};
  const document = {forms:[form]};
  const window = {}; window.top = window;
  const location = new URL(href);
  const context = vm.createContext({document, window, location, URL, isSecureContext:true, HTMLInputElement:Input,
    getComputedStyle:el=>({visibility:el.visible?'visible':'hidden'}), Event:class {constructor(type){this.type=type;}}});
  const fn = vm.runInContext('('+source+')',context);
  return {user,pass,form,document,location,window,Input,context,probe:()=>fn('probe','ticket',origin,href,'',''),
    fill:(ticket='ticket')=>fn('fill',ticket,origin,href,'fixture-user','fixture-password')};
}
test('one selected empty form fills without submitting, ticket consumed',()=>{
  const f=fixture(); assert.equal(f.probe(),true); assert.equal(f.fill(),true);
  assert.equal(f.user.value,'fixture-user'); assert.equal(f.pass.value,'fixture-password');
  assert.deepEqual(f.pass.events,['input','change']); f.user.value=''; f.pass.value=''; assert.equal(f.fill(),false);
});
test('fill without probe or mismatched ticket is rejected',()=>{
  const f=fixture(); assert.equal(f.fill(),false); assert.equal(f.probe(),true);
  assert.equal(f.fill('wrong'),false); assert.equal(f.fill(),false);
});
test('different port, HTTP, frame or insecure context is rejected',()=>{
  for(const change of [f=>f.location.port='8443',f=>f.location.protocol='http:',f=>f.window.top={},f=>f.context.isSecureContext=false]){
    const f=fixture(); change(f); assert.equal(f.probe(),false);
  }
});
test('cross-origin, credentials in action, GET, target and submit overrides rejected',()=>{
  for(const change of [f=>f.form.action='https://other.test/',f=>f.form.action='https://user@example.test/',f=>f.form.method='get',f=>f.form.target='_blank',f=>f.form.override={}]){
    const f=fixture(); change(f); assert.equal(f.probe(),false);
  }
});
test('ambiguous, invisible, disabled, new-password or already filled fields rejected',()=>{
  for(const change of [f=>f.form.elements.push(new f.Input('password','second')),f=>f.pass.visible=false,f=>f.user.disabled=true,f=>f.pass.autocomplete='new-password',f=>f.user.value='existing',f=>f.pass.value='existing',f=>f.document.forms.push({...f.form})]){
    const f=fixture(); change(f); assert.equal(f.probe(),false);
  }
});
test('navigation, form replacement and changed action after probe reject delivery',()=>{
  for(const change of [f=>f.location.pathname='/next',f=>f.document.forms=[{...f.form}],f=>f.form.elements[1]=new f.Input('password','password'),f=>f.form.action='https://other.test/',f=>f.user.value='typed']){
    const f=fixture(); assert.equal(f.probe(),true); change(f); assert.equal(f.fill(),false); assert.equal(f.pass.value,'');
  }
});
