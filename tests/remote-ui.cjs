// Deterministic browser regression tests. All APIs are fixtures; no SendInput or power action.
const {test, before, after} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const {chromium} = require('playwright');

let browser;
before(async()=>{browser=await chromium.launch({headless:true});});
after(async()=>{await browser?.close();});

async function fixture(run, options={}){
  const page=await browser.newPage({viewport:{width:412,height:915},hasTouch:true,isMobile:true});
  const events=[], errors=[], dangerous=[], clipEvents=[], uploads=[];
  const settings={timer_enabled:false,timer_mode:'after_minutes',at_time:'02:00',after_minutes:60,
    trigger_file_enabled:true,filewatch_enabled:true,watch_dirs:'C:\\fixture',file_idle_minutes:3,
    force_close:false,countdown_secs:60,mirror_enabled:true,control_enabled:true};
  const state={phase:options.phase||'config',reason:'',countdown_left:0,fw:{saw_activity:false,ago_secs:null,idle_minutes:3},
    settings,monitors:[{i:0,w:1280,h:720,primary:true}],mirror_clients:1,
    lan_url:'http://192.168.1.10:47823',token:'0123456789abcdef0123456789abcdef',
    tunnel:{status:'running',url:'https://fixture.trycloudflare.com'}};
  if(options.control===false)settings.control_enabled=false;
  let responseStatus=200, delay=0, polls=0;
  page.on('pageerror',e=>errors.push(e.message));
  await page.route('http://shut.test/**',async route=>{
    const request=route.request(), url=new URL(request.url());
    if(url.pathname==='/')return route.fulfill({contentType:'text/html',body:fs.readFileSync(path.join(__dirname,'../ui/index.html'),'utf8')});
    if(url.pathname==='/shell.css'||url.pathname==='/shell.js')return route.fulfill({contentType:url.pathname.endsWith('.css')?'text/css':'application/javascript',body:options.shell?fs.readFileSync(path.join(__dirname,'../ui',url.pathname.slice(1)),'utf8'):''});
    if(url.pathname==='/api/state'){polls++;return route.fulfill({json:state});}
    if(url.pathname==='/input'){
      events.push(request.postDataJSON());
      const status=typeof responseStatus==='function'?responseStatus(events.at(-1)):responseStatus, wait=delay;
      if(wait)await new Promise(r=>setTimeout(r,wait));
      return route.fulfill({status,json:status===200?{ok:true}:{error:'fixture: PC入力が拒否されました'}});
    }
    if(url.pathname==='/api/settings')return route.fulfill({json:{ok:true}});
    if(url.pathname==='/api/clipboard'){
      if(request.method()==='GET')return route.fulfill({json:{ok:true,text:'PC側のコピー'}});
      clipEvents.push(request.postDataJSON());return route.fulfill({json:{ok:true}});
    }
    if(url.pathname==='/api/upload'){uploads.push({name:url.searchParams.get('name'),bytes:(request.postDataBuffer()||Buffer.alloc(0)).length});return route.fulfill({json:{ok:true,path:'C:\\fixture\\incoming\\'+url.searchParams.get('name'),bytes:0}});}
    if(url.pathname.startsWith('/api/')){dangerous.push(url.pathname);return route.fulfill({status:500,json:{error:'power endpoints are forbidden in tests'}});}
    return route.fulfill({contentType:'image/svg+xml',body:'<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720"><rect width="1280" height="720" fill="#182039"/><rect x="100" y="70" width="1080" height="540" rx="20" fill="#e8ecf4"/><text x="150" y="180" font-size="38" fill="#182039">TEST FIXTURE — remote desktop</text><text x="150" y="260" font-size="30" fill="#182039">Japanese text / touch scroll</text></svg>'});
  });
  await page.goto('http://shut.test/?t='+state.token+'&ephemeral=1');
  await page.waitForFunction(()=>document.getElementById('lan_url').textContent.length>0);
  const drain=()=>page.evaluate(()=>{flushPendingTap();return inputTail;});
  const dispatch=(type, props)=>page.locator('#touch').evaluate((el,{type,props})=>{
    // Synthetic pointers exercise the application's handlers, without browser capture state.
    el.setPointerCapture=()=>{};
    el.dispatchEvent(new PointerEvent(type,{bubbles:true,pointerId:1,pointerType:'touch',clientX:140,clientY:180,...props}));
  },{type,props});
  try{
    await run({page,events,errors,state,drain,dispatch,setResponse:(status,ms=0)=>{responseStatus=status;delay=ms;},polls:()=>polls,clipEvents,uploads});
    assert.deepEqual(errors,[]);
    assert.deepEqual(dangerous,[]);
  }finally{await page.close();}
}

test('Japanese IME commit does not send; confirmed text and emoji send once',()=>fixture(async({page,events,drain})=>{
  await page.locator('#kbd').fill('日本語😀');
  await page.locator('#kbd').evaluate(el=>{
    el.dispatchEvent(new CompositionEvent('compositionstart'));
    el.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',isComposing:true,bubbles:true}));
    el.dispatchEvent(new CompositionEvent('compositionend'));
    el.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',keyCode:229,bubbles:true}));
  });
  await drain();assert.equal(events.length,0);
  await page.waitForTimeout(250); // 確定直後のEnterガード期間を越えてから送信する
  await page.locator('#kbd').press('Enter');await drain();
  assert.equal(events.length,1);assert.equal(events[0].text,'日本語😀');
  assert.equal(await page.locator('#kbd').inputValue(),'');
}));

test('failed send preserves the draft and displays the server error',()=>fixture(async({page,events,drain,setResponse})=>{
  setResponse(422);await page.locator('#kbd').fill('失敗しても残す');
  await page.locator('#kbd_send').click();await drain();
  assert.equal(events.length,1);assert.equal(await page.locator('#kbd').inputValue(),'失敗しても残す');
  assert.match(await page.locator('#input_status').textContent(),/拒否/);
  assert.equal(await page.locator('#input_status').getAttribute('class'),'input-status error');
}));

test('acknowledgement clears only the sent draft; Shift Enter inserts a newline',()=>fixture(async({page,events,drain,setResponse})=>{
  setResponse(200,180);await page.locator('#kbd').fill('一行');await page.locator('#kbd').press('Shift+Enter');
  await page.keyboard.insertText('二');
  await page.locator('#kbd_send').click();
  await page.locator('#kbd').fill('送信中に書いた次の文章');await drain();
  assert.match(events[0].text,/一行\n/);
  assert.equal(await page.locator('#kbd').inputValue(),'送信中に書いた次の文章');
}));

test('touch swipes use natural direction; ordinary wheel direction is retained',()=>fixture(async({page,events,dispatch,drain})=>{
  await dispatch('pointerdown',{clientY:220});await dispatch('pointermove',{clientY:160});await dispatch('pointerup',{clientY:160});await drain();
  assert.equal(events[0].type,'scroll');assert.ok(events[0].dy>0,'swipe up scrolls down');
  await page.waitForTimeout(55);
  await dispatch('pointerdown',{clientY:160});await dispatch('pointermove',{clientY:220});await dispatch('pointerup',{clientY:220});await drain();
  assert.ok(events[1].dy<0,'swipe down scrolls up');
  await page.locator('#touch').dispatchEvent('wheel',{deltaY:120,deltaMode:0,clientX:150,clientY:180});await drain();
  assert.equal(events[2].dy,1);
}));

test('double tap uses one atomic double click; cancelling or pinching a drag releases the button',()=>fixture(async({page,events,dispatch,drain})=>{
  for(let i=0;i<2;i++){await dispatch('pointerdown',{});await dispatch('pointerup',{});}await drain();
  assert.equal(events.length,1);assert.equal(events[0].type,'double_click');
  events.length=0;await page.locator('#mode_btn').click();
  await dispatch('pointerdown',{});await dispatch('pointermove',{clientX:180});await dispatch('pointercancel',{clientX:180});await drain();
  assert.deepEqual(events.map(e=>e.type),['down','move','up']);
  events.length=0;await page.waitForTimeout(55);
  await dispatch('pointerdown',{});await dispatch('pointermove',{clientX:180});await dispatch('pointerdown',{pointerId:2,clientX:230});await drain();
  assert.deepEqual(events.map(e=>e.type),['down','move','up']);
}));

test('double tap stays atomic with a slow response, and a pending tap precedes text',()=>fixture(async({page,events,dispatch,drain,setResponse})=>{
  setResponse(200,900);
  await dispatch('pointerdown',{});await dispatch('pointerup',{});
  await page.waitForTimeout(100);
  await dispatch('pointerdown',{});await dispatch('pointerup',{});await drain();
  assert.deepEqual(events.map(e=>e.type),['double_click']);
  events.length=0;setResponse(200,0);
  await dispatch('pointerdown',{});await dispatch('pointerup',{});
  await page.evaluate(()=>inp({type:'text',text:'タップした入力先'}));await drain();
  assert.deepEqual(events.map(e=>e.type),['click','text']);
}));

test('queue saturation never discards release; unsent pointer motion is coalesced',()=>fixture(async({page,events,drain,setResponse})=>{
  setResponse(200,30);
  await page.evaluate(()=>{
    inp({type:'down',x:0.1,y:0.1});
    for(let i=0;i<100;i++)inp({type:'move',x:i/100,y:0.2});
    inp({type:'up',x:0.99,y:0.2});
  });await drain();
  assert.deepEqual(events.map(e=>e.type),['down','move','up']);
  assert.equal(events[1].x,0.99);
  events.length=0;
  await page.evaluate(()=>{
    inp({type:'down',x:0.1,y:0.1});
    for(let i=0;i<63;i++)inp({type:'key',key:'Space'});
    inp({type:'up',x:0.2,y:0.2});
  });await drain();
  assert.equal(events.length,65);assert.equal(events.at(-1).type,'up');
}));

test('failed focus click blocks dependent text and preserves the draft',()=>fixture(async({page,events,dispatch,drain,setResponse})=>{
  setResponse(event=>event.type==='click'?422:200);
  await dispatch('pointerdown',{});await dispatch('pointerup',{});
  await page.locator('#kbd').fill('別の入力先へ送らない');
  await page.locator('#kbd_send').click();await drain();
  assert.deepEqual(events.map(e=>e.type),['click']);
  assert.equal(await page.locator('#kbd').inputValue(),'別の入力先へ送らない');
  assert.match(await page.locator('#input_status').textContent(),/入力先を選択できませんでした/);
}));

test('input requests wait for earlier responses, maintaining key order',()=>fixture(async({page,events,drain,setResponse})=>{
  setResponse(200,150);
  await page.evaluate(()=>{inp({type:'text',text:'先'});inp({type:'key',key:'Enter'});inp({type:'text',text:'後'});});
  await page.waitForTimeout(50);assert.equal(events.length,1);
  await drain();assert.deepEqual(events.map(e=>e.text||e.key),['先','Enter','後']);
}));

test('desktop mouse drag, right click and physical shortcut are forwarded',()=>fixture(async({page,events,dispatch,drain})=>{
  await dispatch('pointerdown',{pointerType:'mouse',button:0});
  await dispatch('pointermove',{pointerType:'mouse',clientX:190});
  await dispatch('pointerup',{pointerType:'mouse',clientX:190});await drain();
  assert.deepEqual(events.map(e=>e.type),['down','move','up']);
  events.length=0;
  await dispatch('pointerdown',{pointerType:'mouse',button:2});await dispatch('pointerup',{pointerType:'mouse',button:2});await drain();
  assert.equal(events[0].button,'right');
  await page.locator('#touch').dispatchEvent('keydown',{key:'c',ctrlKey:true});await drain();
  assert.deepEqual(events.at(-1).modifiers,['Control']);assert.equal(events.at(-1).key,'c');
}));

test('a running tunnel continues polling; ephemeral mode stores no token; mobile layout fits',()=>fixture(async({page,polls})=>{
  await page.waitForTimeout(2150);assert.ok(polls()>=3);
  assert.equal(await page.locator('#btn_tunnel').textContent(),'停止');
  assert.equal(await page.evaluate(()=>localStorage.getItem('shut_t')),null);
  assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
  const output=path.join(__dirname,'../target/verification');fs.mkdirSync(output,{recursive:true});
  await page.locator('#screen').locator('..').screenshot({path:path.join(output,'remote-mobile.png')});
}));

test('view-only mode disables text and special keys',()=>fixture(async({page,events})=>{
  assert.ok(await page.locator('#kbd_send').isDisabled());
  assert.ok(await page.locator('[data-k="Enter"]').isDisabled());
  const ok=await page.evaluate(()=>inp({type:'text',text:'送らない'}));
  assert.equal(ok,false);assert.equal(events.length,0);
},{control:false}));

test('countdown preview and cancelling preview never call a power endpoint',()=>fixture(async({page})=>{
  await page.locator('#btn_test').click();
  assert.match(await page.locator('#cdreason').textContent(),/実行しません/);
  assert.ok(await page.locator('#cd_now').isHidden());
  await page.waitForTimeout(1100);
  assert.ok(await page.locator('#cd').isVisible());
  await page.locator('#cd_cancel').click();
  assert.ok(await page.locator('#cd').isHidden());
}));

test('mobile IME send key (beforeinput paragraph) sends the draft once',()=>fixture(async({page,events,drain})=>{
  await page.locator('#kbd').fill('ソフトキーボードの送信');
  await page.locator('#kbd').evaluate(el=>el.dispatchEvent(new InputEvent('beforeinput',{inputType:'insertParagraph',cancelable:true,bubbles:true})));
  await drain();
  assert.equal(events.length,1);assert.equal(events[0].type,'text');assert.equal(events[0].text,'ソフトキーボードの送信');
  await page.locator('#kbd').fill('次の文');
  await page.locator('#kbd').evaluate(el=>{
    el.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',shiftKey:true,bubbles:true}));
    el.dispatchEvent(new InputEvent('beforeinput',{inputType:'insertParagraph',cancelable:true,bubbles:true}));
  });
  await drain();
  assert.equal(events.length,1,'Shift+改行は送信しない');
  assert.match(await page.locator('#kbd').inputValue(),/次の文/);
}));

test('live mode forwards typed diffs, backspace and enter immediately',()=>fixture(async({page,events,drain})=>{
  await page.locator('#kmode_live').click();
  const live=page.locator('#live_kbd');
  await live.click();
  await live.pressSequentially('ab');
  await live.press('Backspace');
  await drain();
  assert.equal(events.filter(e=>e.type==='text').map(e=>e.text).join(''),'ab');
  assert.equal(events.filter(e=>e.type==='key'&&e.key==='Backspace').length,1);
  events.length=0;
  await live.press('Enter');
  await drain();
  assert.deepEqual(events.map(e=>e.key),['Enter']);
  events.length=0;
  await page.locator('#klive').evaluate(el=>el.dispatchEvent(new Event('submit',{cancelable:true,bubbles:true})));
  await drain();
  assert.deepEqual(events.map(e=>e.key),['Enter']);
}));

test('live mode IME commit sends once even when the field is rewritten afterwards',()=>fixture(async({page,events,drain})=>{
  await page.locator('#kmode_live').click();
  const live=page.locator('#live_kbd');
  await live.click();
  await live.evaluate(el=>{
    const seed=el.value;
    el.dispatchEvent(new CompositionEvent('compositionstart',{bubbles:true}));
    el.value=seed+'にほ';
    el.dispatchEvent(new InputEvent('input',{data:'にほ',isComposing:true,bubbles:true}));
    el.value=seed+'にほん';
    el.dispatchEvent(new InputEvent('input',{data:'ん',isComposing:true,bubbles:true}));
    el.dispatchEvent(new CompositionEvent('compositionend',{data:'にほん',bubbles:true}));
    el.dispatchEvent(new InputEvent('input',{data:'にほん',isComposing:false,bubbles:true}));
  });
  await drain();
  assert.equal(events.length,1);assert.equal(events[0].text,'にほん');
  // IMEが確定後に同じ文字列を書き戻しても再送しない（値が同じなら差分なしとして扱う）
  await live.evaluate(el=>el.dispatchEvent(new InputEvent('input',{data:'にほん',bubbles:true})));
  await drain();
  assert.equal(events.length,1,'書き戻しで二重送信しない');
  // 値が残っていても、次の入力は差分として正しく送れる
  await live.evaluate(el=>{el.value=el.value+'x';el.dispatchEvent(new InputEvent('input',{data:'x',bubbles:true}));});
  await drain();
  assert.equal(events.at(-1).text,'x');
}));

test('live mode discards uncommitted composition on cancel and blur',()=>fixture(async({page,events,drain})=>{
  await page.locator('#kmode_live').click();
  const live=page.locator('#live_kbd');
  await live.click();
  await live.evaluate(el=>{
    const seed=el.value;
    el.dispatchEvent(new CompositionEvent('compositionstart',{bubbles:true}));
    el.value=seed+'かな';
    el.dispatchEvent(new InputEvent('input',{data:'かな',isComposing:true,bubbles:true}));
    el.dispatchEvent(new CompositionEvent('compositioncancel',{bubbles:true}));
    el.dispatchEvent(new InputEvent('input',{inputType:'insertCompositionText',bubbles:true}));
  });
  await drain();
  assert.equal(events.length,0,'キャンセルされた合成は送らない');
  // blur後の書き戻しも送らない
  await live.click();
  await live.evaluate(el=>{
    const seed=el.value;
    el.dispatchEvent(new CompositionEvent('compositionstart',{bubbles:true}));
    el.value=seed+'てす';
    el.dispatchEvent(new InputEvent('input',{data:'てす',isComposing:true,bubbles:true}));
  });
  await live.blur();
  await live.evaluate(el=>el.dispatchEvent(new InputEvent('input',{data:'てすと',bubbles:true})));
  await drain();
  assert.equal(events.length,0,'blur中の未確定文字は送らない');
}));

test('live mode treats Enter right after an IME commit as part of the commit',()=>fixture(async({page,events,drain})=>{
  await page.locator('#kmode_live').click();
  const live=page.locator('#live_kbd');
  await live.click();
  await live.evaluate(el=>{
    const seed=el.value;
    el.dispatchEvent(new CompositionEvent('compositionstart',{bubbles:true}));
    el.value=seed+'かんじ';
    el.dispatchEvent(new InputEvent('input',{data:'かんじ',isComposing:true,bubbles:true}));
    el.value=seed+'漢字';
    el.dispatchEvent(new InputEvent('input',{data:'漢字',isComposing:true,bubbles:true}));
    el.dispatchEvent(new CompositionEvent('compositionend',{data:'漢字',bubbles:true}));
    // 確定を兼ねたEnterがcompositionend直後にkeydownで届くIMEがある
    el.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',keyCode:13,bubbles:true}));
    el.dispatchEvent(new Event('submit',{cancelable:true,bubbles:true}));
  });
  await live.evaluate(el=>el.form.dispatchEvent(new Event('submit',{cancelable:true,bubbles:true})));
  await page.waitForTimeout(80); // compositionend後の遅延同期(40ms)を待つ
  await drain();
  assert.deepEqual(events.filter(e=>e.type==='text').map(e=>e.text),['漢字']);
  assert.equal(events.filter(e=>e.key==='Enter').length,0,'確定Enterで改行を送らない');
  // 時間を置いたEnterは普通に送信される
  await page.waitForTimeout(250);
  await live.press('Enter');
  await drain();
  assert.deepEqual(events.filter(e=>e.key==='Enter').map(e=>e.key),['Enter']);
}));

test('batch send ignores the Enter that only commits an IME composition',()=>fixture(async({page,events,drain})=>{
  const ta=page.locator('#kbd');
  await ta.click();
  await ta.evaluate(el=>{
    el.dispatchEvent(new CompositionEvent('compositionstart',{bubbles:true}));
    el.value='にほんご';
    el.dispatchEvent(new InputEvent('input',{data:'にほんご',isComposing:true,bubbles:true}));
    el.value='日本語';
    el.dispatchEvent(new InputEvent('input',{data:'日本語',isComposing:true,bubbles:true}));
    el.dispatchEvent(new CompositionEvent('compositionend',{data:'日本語',bubbles:true}));
    // 確定Enter由来の改行要求（合成直後は送信しない）
    el.dispatchEvent(new InputEvent('beforeinput',{inputType:'insertParagraph',cancelable:true,bubbles:true}));
    el.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',keyCode:13,bubbles:true}));
  });
  await drain();
  assert.equal(events.length,0,'確定Enterで途中送信しない');
  assert.match(await ta.inputValue(),/日本語/);
  await page.waitForTimeout(250);
  await ta.evaluate(el=>el.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',keyCode:13,bubbles:true})));
  await drain();
  assert.equal(events.length,1);assert.equal(events[0].text,'日本語');
}));

test('live mode does not wipe a new composition started right after a send',()=>fixture(async({page,events,drain})=>{
  await page.locator('#kmode_live').click();
  const live=page.locator('#live_kbd');
  await live.click();
  await live.pressSequentially('a');
  await drain();
  assert.equal(events.filter(e=>e.type==='text').map(e=>e.text).join(''),'a');
  // 送信直後（リセット猶予中）に次の合成を開始してもフィールドが消えない
  await live.evaluate(el=>{
    el.dispatchEvent(new CompositionEvent('compositionstart',{bubbles:true}));
    el.value=el.value+'つ';
    el.dispatchEvent(new InputEvent('input',{data:'つ',isComposing:true,bubbles:true}));
  });
  await page.waitForTimeout(150); // リセット猶予(60ms)をまたいでも消えない
  assert.match(await live.inputValue(),/つ/);
  await live.evaluate(el=>{
    el.value=el.value.slice(0,-1)+'次'; // 合成中の「つ」が確定で「次」に置き換わる
    el.dispatchEvent(new InputEvent('input',{data:'次',isComposing:true,bubbles:true}));
    el.dispatchEvent(new CompositionEvent('compositionend',{data:'次',bubbles:true}));
  });
  await page.waitForTimeout(80); // compositionend後の遅延同期(40ms)を待つ
  await drain();
  assert.equal(events.at(-1).text,'次','合成の確定が送られる');
}));

test('live mode handles emoji without splitting surrogate pairs',()=>fixture(async({page,events,drain})=>{
  await page.locator('#kmode_live').click();
  const live=page.locator('#live_kbd');
  await live.click();
  await live.pressSequentially('😀');
  await drain();
  const texts=events.filter(e=>e.type==='text').map(e=>e.text);
  assert.equal(texts.join(''),'😀');
  assert.equal([...texts.join('')].length,1,'絵文字は1コードポイントで届く');
  events.length=0;
  await live.press('Backspace');
  await drain();
  assert.equal(events.filter(e=>e.key==='Backspace').length,1,'絵文字1文字はBackspace1回');
}));

test('armed state keeps remote input controls usable',()=>fixture(async({page})=>{
  await page.waitForTimeout(2500);
  assert.ok(await page.evaluate(()=>!document.getElementById('live_kbd').disabled),'live_kbd is enabled while armed');
  assert.ok(await page.evaluate(()=>!document.getElementById('file_in').disabled),'file_in is enabled while armed');
  assert.ok(await page.evaluate(()=>document.getElementById('s_timer').disabled),'settings stay locked while armed');
},{phase:'armed'}));

test('clipboard get fills the batch composer; clipboard set posts the text',()=>fixture(async({page,clipEvents})=>{
  await page.locator('#clip_get').click();
  await page.waitForFunction(()=>document.getElementById('kbd').value==='PC側のコピー');
  await page.locator('#kbd').fill('端末からのコピー');
  await page.locator('#clip_set').click();
  await page.waitForFunction(()=>document.getElementById('input_status').textContent.includes('入れました'));
  assert.equal(clipEvents.length,1);assert.equal(clipEvents[0].text,'端末からのコピー');
}));

test('file pick uploads raw bytes with the file name',()=>fixture(async({page,uploads})=>{
  await page.locator('#file_in').setInputFiles({name:'写真.png',mimeType:'image/png',buffer:Buffer.from([1,2,3,4])});
  await page.waitForFunction(()=>document.getElementById('input_status').textContent.includes('保存'));
  assert.equal(uploads.length,1);assert.equal(uploads[0].name,'写真.png');assert.equal(uploads[0].bytes,4);
}));

test('unfocused keyboard input reaches the PC until a panel or field takes focus',()=>fixture(async({page,events,drain})=>{
  await page.locator('body').press('x');
  await drain();
  assert.equal(events.at(-1).type,'text');assert.equal(events.at(-1).text,'x');
  await page.locator('#kbd').focus();
  await page.keyboard.press('y');
  await drain();
  assert.equal(events.length,1,'フィールド内のキーは転送しない');
}));

test('shell workbench boots, strip and quick keys render and function',()=>fixture(async({page,events,drain})=>{
  assert.ok(await page.locator('.sh-workbench').isVisible());
  assert.ok(await page.locator('.sh-strip').isVisible());
  assert.equal(await page.locator('.sh-qkey').count(),4);
  await page.locator('.sh-qkey').first().click();await drain();
  assert.equal(events.at(-1).type,'key');assert.equal(events.at(-1).key,'Enter');
  // リロードボタンがストリームを張り直す（rパラメータ付きの新規要求が出る）
  const[reloadReq]=await Promise.all([
    page.waitForRequest(r=>r.url().includes('/stream.mjpeg')&&r.url().includes('r=')),
    page.locator('#sh-reload').click(),
  ]);
  assert.ok(reloadReq.url().includes('m=0'));
  // マウスドックを開いても画面を覆わない（ドロワーは出ず、パッドが下端にドッグする）
  await page.locator('#sh-dock-pointer').click();
  await page.waitForSelector('.sh-paneldock:not([hidden])');
  assert.ok(!(await page.locator('.sh-drawer').isVisible()));
  const canvasH=await page.evaluate(()=>document.querySelector('.sh-canvas').getBoundingClientRect().height);
  assert.ok(canvasH>200,'canvas stays visible while the pad is docked');
  assert.ok(await page.locator('#sh-pad').isVisible());
  const output=path.join(__dirname,'../target/verification');fs.mkdirSync(output,{recursive:true});
  await page.screenshot({path:path.join(output,'shell-pad-dock.png')});
  // 文字パネルもドロワーではなく下端ドッグで開き、画面を覆わない
  await page.locator('#sh-dock-text').click();
  await page.waitForSelector('.sh-paneldock:not([hidden])');
  assert.ok(!(await page.locator('.sh-drawer').isVisible()));
  assert.ok(await page.locator('#kbd').isVisible());
  const canvasH2=await page.evaluate(()=>document.querySelector('.sh-canvas').getBoundingClientRect().height);
  assert.ok(canvasH2>120,'canvas stays visible while the text panel is docked');
  await page.locator('#sh-dock-text').click();
  assert.ok(!(await page.locator('.sh-paneldock').isVisible()));
  await page.screenshot({path:path.join(output,'shell-mobile.png')});
  await page.setViewportSize({width:1280,height:800});
  await page.waitForTimeout(300);
  await page.screenshot({path:path.join(output,'shell-desktop.png')});
  assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
},{shell:true}));
