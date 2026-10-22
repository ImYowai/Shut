/* Progressive shell: keep the existing controls and their event handlers. */
(()=>{
  'use strict';
  const boot=()=>{
    const $=id=>document.getElementById(id);
    const root=document.querySelector('.wrap');
    if(!root || $('sh-workbench') || !window.shutControls)return;
    const controls=window.shutControls;
    const icon=name=>{
      const paths={screen:'<rect x="3" y="4" width="18" height="13" rx="3"/><path d="M8 21h8M12 17v4"/>',connect:'<path d="M9 8V5a3 3 0 0 1 6 0v3M6 8h12v7a6 6 0 0 1-12 0zM12 21v2"/>',timer:'<circle cx="12" cy="13" r="8"/><path d="M12 9v5l3 2M9 2h6M12 2v3"/>',settings:'<path d="m10 3-1 3-3 1-3-1-1 4 3 2v3l-1 3 4 2 2-2h3l3 1 2-4-2-2v-3l1-3-4-2-2 2z"/><circle cx="11" cy="12" r="3"/>',pointer:'<path d="M6 3v17l5-5 4 7 3-2-4-7h7z"/>',reload:'<path d="M20 11a8 8 0 1 0-1.1 4"/><path d="M20 5v6h-6"/>',text:'<path d="M4 5h16M12 5v15M8 20h8M4 5v4M20 5v4"/>',tools:'<rect x="3" y="5" width="18" height="15" rx="3"/><path d="M8 3v4M16 3v4M3 10h18M8 14h1M15 14h1M8 17h1M15 17h1"/>',close:'<path d="m6 6 12 12M18 6 6 18"/>',arrow:'<path d="M5 12h14m-5-5 5 5-5 5"/>',mouse:'<rect x="6" y="2" width="12" height="20" rx="6"/><path d="M12 3v6M6 10h12"/>'};
      return `<svg class="sh-icon" viewBox="0 0 24 24" aria-hidden="true">${paths[name]||paths.screen}</svg>`;
    };
    const make=(tag,cls,id)=>{const el=document.createElement(tag);if(cls)el.className=cls;if(id)el.id=id;return el;};
    const button=(cls,id,label,svg)=>{const b=make('button',cls,id);b.type='button';b.title=label;b.setAttribute('aria-label',label);b.innerHTML=icon(svg)+`<span>${label}</span>`;return b;};
    const header=root.querySelector('header');
    const triggerCard=$('s_timer').closest('.card');
    const connectionCard=$('lan_url').closest('.card');
    const remoteCard=$('screen').closest('.card');
    const statusCard=$('status_card');
    const composer=remoteCard.querySelector('.composer');
    const keyRows=[...remoteCard.querySelectorAll('.kbdrow')];
    const shutdownRow=remoteCard.querySelector('.shutdown-row');
    const displayTools=$('show_mirror').closest('.row');
    const permissionRow=$('s_mirror').closest('.row');
    const remoteHelp=remoteCard.querySelector('.remote-help');
    const screen=$('screen'),view=$('view'),image=$('mjpeg');
    document.body.classList.add('sh-shell');
    header.classList.add('sh-header');
    const brand=make('span','sh-brandmark');brand.setAttribute('aria-hidden','true');
    brand.innerHTML='<svg viewBox="0 0 32 32" fill="none"><rect x="3" y="3" width="26" height="26" rx="10" fill="#dce7d5"/><path d="M11 10v11h11" stroke="#678b78" stroke-width="3" stroke-linecap="round"/><circle cx="21" cy="11" r="3" fill="#df875d"/></svg>';
    header.prepend(brand);
    const caption=make('span','sh-header-caption');caption.textContent='小さな操作室';header.querySelector('h1').after(caption);
    const connectQuick=button('sh-header-action','sh-connect-quick','接続','connect');
    connectQuick.querySelector('span').remove();header.append(connectQuick);
    const workbench=make('div','sh-workbench','sh-workbench');
    const rail=make('nav','sh-rail','sh-rail');rail.setAttribute('aria-label','操作室メニュー');
    const stage=make('main','sh-stage','sh-stage');stage.setAttribute('aria-label','リモート画面');
    const stageBar=make('div','sh-stagebar');
    const stageLabel=make('span','sh-stage-label');stageLabel.textContent='REMOTE DESKTOP';
    const grow=make('span','sh-grow');
    const reloadBtn=button('sh-stage-action','sh-reload','画面を再接続','reload');reloadBtn.querySelector('span').remove();
    reloadBtn.onclick=()=>{navigator.vibrate?.(4);controls.reload?.();};
    stageBar.append(stageLabel,grow,$('seg_mon'),reloadBtn,$('fs_btn'));
    const canvas=make('div','sh-canvas','sh-canvas');
    canvas.append(screen);
    const notice=make('div','sh-stage-notice','sh-stage-notice');
    notice.innerHTML=icon('screen')+'<strong>画面を待っています</strong><span>接続したPCの画面が、ここに届きます。</span>';
    canvas.append(notice);
    const flags=make('div','sh-stage-flags');flags.append($('mir_off'),$('ctrl_off'));canvas.append(flags);
    const cursor=make('div','sh-cursor','sh-cursor');cursor.hidden=true;cursor.setAttribute('aria-hidden','true');
    cursor.innerHTML='<svg viewBox="0 0 24 29"><path d="M2 1v22l6-6 5 10 4-2-5-10h9z"/></svg>';view.append(cursor);
    // 画面を見ながら使うパネル（マウス/文字/ツール）は下端の常設ドッグで開き、画面を覆わない
    const DOCK_PANELS=new Set(['pointer','text','tools']);
    const panelDock=make('div','sh-paneldock','sh-paneldock');panelDock.hidden=true;
    const dock=make('nav','sh-dock','sh-dock');dock.setAttribute('aria-label','リモート操作');
    const backdrop=make('button','sh-backdrop','sh-backdrop');backdrop.type='button';backdrop.tabIndex=-1;backdrop.setAttribute('aria-label','パネルを閉じる');
    const drawer=make('aside','sh-drawer','sh-drawer');drawer.hidden=true;drawer.setAttribute('role','dialog');drawer.setAttribute('aria-modal','true');drawer.setAttribute('aria-labelledby','sh-panel-title');
    const drawerHead=make('div','sh-drawer-head');
    const titleBlock=make('div');titleBlock.style.flex='1';titleBlock.innerHTML='<div class="sh-drawer-kicker">YOUR LITTLE CONTROL ROOM</div><h2 class="sh-drawer-title" id="sh-panel-title"></h2>';
    const close=button('sh-close','sh-close','閉じる','close');close.querySelector('span').remove();drawerHead.append(titleBlock,close);
    const drawerBody=make('div','sh-drawer-body','sh-drawer-body');drawer.append(drawerHead,drawerBody);
    const panes={};
    for(const name of ['connect','timer','settings','pointer','text','tools']){panes[name]=make('section','sh-pane',`sh-pane-${name}`);panes[name].hidden=true;drawerBody.append(panes[name]);}
    stage.append(stageBar,canvas,panelDock,dock);workbench.append(rail,stage,backdrop,drawer);root.append(workbench);
    connectionCard.classList.add('sh-connect-card');triggerCard.classList.add('sh-timer-card');permissionRow.classList.add('sh-permission-row');displayTools.classList.add('sh-display-tools');
    const art=make('div','sh-connect-art');art.setAttribute('aria-hidden','true');
    art.innerHTML='<svg viewBox="0 0 220 120" fill="none"><ellipse cx="112" cy="100" rx="72" ry="12" fill="#dce3d1"/><path d="m34 73 75-38 79 39-74 39z" fill="#d9dfce"/><path d="m34 73 80 39v-9L34 64z" fill="#c3cfba"/><path d="m34 64 75-38 79 39-74 38z" fill="#f9f7ec"/><path d="m75 39 43-21 37 18v36l-42 20-38-18z" fill="#a4b99e"/><path d="m82 41 34-16 30 15v23l-33 16-31-15z" fill="#657e70"/><path d="m87 44 29-14 24 12v16l-27 13-26-13z" fill="#e5ecdb"/><path d="m111 79 8-4v13l-8 4z" fill="#869d7d"/><path d="m99 91 15-7 15 7-15 7z" fill="#b2c2a8"/><path d="m145 83 14-7 16 8-14 7z" fill="#df875d"/><path d="m145 83 16 8v9l-16-8z" fill="#ca7953"/><path d="m161 91 14-7v9l-14 7z" fill="#bc704c"/><circle cx="62" cy="61" r="9" fill="#a9bf91"/><path d="M62 65v17" stroke="#8da47c" stroke-width="4" stroke-linecap="round"/></svg>';
    const connectCaption=make('p','sh-connect-caption');connectCaption.textContent='QRを読み取って、このPCとつながる。';
    panes.connect.append(art,connectCaption,connectionCard);
    panes.timer.append(triggerCard,statusCard);
    const settingsIntro=make('p','sh-panel-intro');settingsIntro.textContent='このPCで使える操作を選びます。';
    panes.settings.append(settingsIntro,permissionRow,shutdownRow);
    panes.text.append(composer);
    const displaySection=make('div');displaySection.innerHTML='<div class="sh-section-label">画面と操作</div>';displaySection.append(displayTools);
    const keySection=make('div');keySection.innerHTML='<div class="sh-section-label">キー</div>';keySection.append(...keyRows);
    keyRows.forEach((row,i)=>{if(i)row.style.marginTop='8px';});
    const shortcutSection=make('div');shortcutSection.innerHTML='<div class="sh-section-label">ショートカット</div>';
    const shortcuts=make('div','sh-shortcuts');
    for(const [label,key] of [['全選択','A'],['コピー','C'],['貼り付け','V'],['元に戻す','Z']]){
      const b=make('button','sh-button sh-remote-action');b.type='button';b.textContent=label;b.title=`Control + ${key}`;
      b.addEventListener('click',()=>sendRemote({type:'key',key,modifiers:['Control']}));shortcuts.append(b);
    }
    shortcutSection.append(shortcuts);
    const toolLinks=make('div','sh-tools-links');
    const menuDefs=[['connect','接続','connect'],['timer','タイマー','timer'],['settings','設定','settings']];
    for(const [name,label,svg] of menuDefs){const b=button('sh-button sh-tools-link',`sh-tool-${name}`,label,svg);b.onclick=()=>openPanel(name,b);toolLinks.append(b);}
    panes.tools.append(displaySection,keySection,shortcutSection,toolLinks,remoteHelp);
    remoteCard.remove();
    const footer=root.querySelector('footer');if(footer)footer.hidden=true;
    const navButtons=[];
    for(const [name,label,svg] of [['screen','画面','screen'],...menuDefs]){
      const b=button('sh-nav-button',`sh-nav-${name}`,label,svg);b.dataset.panel=name;b.setAttribute('aria-pressed',name==='screen'?'true':'false');
      b.onclick=()=>name==='screen'?closePanel():togglePanel(name,b);rail.append(b);navButtons.push(b);
    }
    const railNote=make('span','sh-rail-note');railNote.textContent='SHUT / YOUR DESKTOP';rail.append(railNote);
    for(const [name,label,svg] of [['pointer','マウス','pointer'],['text','文字','text'],['tools','ツール','tools']]){
      const b=button('sh-dock-button',`sh-dock-${name}`,label,svg);b.dataset.panel=name;b.setAttribute('aria-pressed','false');b.setAttribute('aria-controls','sh-drawer');b.setAttribute('aria-expanded','false');
      b.onclick=()=>togglePanel(name,b);dock.append(b);navButtons.push(b);
    }
    const pad=make('div','sh-pad','sh-pad');pad.tabIndex=0;pad.setAttribute('role','application');pad.setAttribute('aria-label','仮想トラックパッド。指でカーソルを動かし、タップでクリック、2本指でスクロール');
    pad.innerHTML=`<div class="sh-pad-center">${icon('mouse')}<span>指で、そっと動かす</span></div>`;
    const padActions=make('div','sh-pad-actions');
    const left=make('button','sh-button sh-remote-action','sh-pad-left');left.type='button';left.textContent='左クリック';
    const drag=make('button','sh-button sh-remote-action','sh-pad-drag');drag.type='button';drag.textContent='ドラッグ';drag.setAttribute('aria-pressed','false');
    const right=make('button','sh-button sh-remote-action','sh-pad-right');right.type='button';right.textContent='右クリック';
    padActions.append(left,drag,right);
    const padHelp=make('div','sh-pad-help');padHelp.innerHTML='<span>タップでクリック · 長押しで右クリック</span><span>2本指でスクロール</span>';
    const padStatus=make('div','input-status','sh-pad-status');padStatus.setAttribute('role','status');padStatus.setAttribute('aria-live','polite');
    panes.pointer.append(pad,padActions,padHelp,padStatus);
    // 画面端のスクロールストリップ。指の向き=スクロール方向の、分かりにくさのない物理ホイール感覚。
    const strip=make('div','sh-strip','sh-strip');strip.tabIndex=0;strip.setAttribute('role','scrollbar');strip.setAttribute('aria-label','スクロールバー');strip.setAttribute('aria-orientation','vertical');
    strip.innerHTML='<span class="sh-strip-thumb" aria-hidden="true"></span>';screen.append(strip);
    // 画面右下のクイックキー（Enter/Esc/Tab/Backspace）。エミュレータのショルダー的な定番キー。
    const keys=make('div','sh-quickkeys','sh-quickkeys');
    for(const [label,key,title] of [['⏎','Enter','Enter'],['⎋','Escape','Esc'],['⇥','Tab','Tab'],['⌫','Backspace','Backspace']]){
      const b=make('button','sh-qkey sh-remote-action');b.type='button';b.textContent=label;b.title=title;b.setAttribute('aria-label',title);
      b.addEventListener('click',()=>{navigator.vibrate?.(6);sendRemote({type:'key',key});});
      keys.append(b);
    }
    screen.append(keys);
    let panel=null,lastOpener=null,lastState=controls.state?.(),loaded=false;
    const panelTitles={connect:'つなぐ',timer:'タイマーと見守り',settings:'操作室の設定',pointer:'マウスを動かす',text:'文字を送る',tools:'操作の道具'};
    function syncNav(){
      navButtons.forEach(b=>{const on=b.dataset.panel===(panel||'screen');b.setAttribute('aria-pressed',String(on));if(b.classList.contains('sh-dock-button'))b.setAttribute('aria-expanded',String(on));});
      connectQuick.setAttribute('aria-expanded',String(panel==='connect'));
    }
    function openPanel(name,opener){
      if(!panes[name])return;
      if(panel==='pointer'){flushPadTap();releaseDrag();resetPad();}
      lastOpener=opener||document.activeElement;
      if(DOCK_PANELS.has(name)){
        drawer.hidden=true;workbench.classList.remove('sh-panel-open');
        panel=name;
        Object.keys(panes).forEach(k=>{if(DOCK_PANELS.has(k))panes[k].hidden=k!==name;});
        panelDock.append(panes[name]);panelDock.hidden=false;workbench.classList.add('sh-dock-open');
        syncNav();requestAnimationFrame(fitScreen);
        (name==='text'?$('kbd'):name==='pointer'?pad:panes[name].querySelector('button'))?.focus({preventScroll:true});
        return;
      }
      panelDock.hidden=true;workbench.classList.remove('sh-dock-open');
      panel=name;drawer.hidden=false;drawer.dataset.panel=name;workbench.classList.add('sh-panel-open');
      Object.entries(panes).forEach(([key,pane])=>{pane.hidden=key!==name;});$('sh-panel-title').textContent=panelTitles[name];drawerBody.scrollTop=0;syncNav();
      requestAnimationFrame(()=>{fitScreen();close.focus({preventScroll:true});});
    }
    function closePanel(){
      if(!panel)return;
      if(panel==='pointer'){flushPadTap();releaseDrag();resetPad();}
      if(DOCK_PANELS.has(panel)){panes[panel].hidden=true;panelDock.hidden=true;workbench.classList.remove('sh-dock-open');}
      else{drawer.hidden=true;workbench.classList.remove('sh-panel-open');}
      panel=null;syncNav();fitScreen();
      if(lastOpener?.isConnected)lastOpener.focus({preventScroll:true});
    }
    function togglePanel(name,opener){if(panel===name)closePanel();else openPanel(name,opener);}
    close.onclick=closePanel;backdrop.onclick=closePanel;connectQuick.onclick=()=>togglePanel('connect',connectQuick);
    document.addEventListener('keydown',e=>{
      if(!panel||$('auth')?.classList.contains('show')||$('cd')?.classList.contains('show'))return;
      if(e.key==='Escape'){e.preventDefault();closePanel();return;}
      if(e.key!=='Tab')return;
      const host=DOCK_PANELS.has(panel)?panelDock:drawer;
      const candidates=[...host.querySelectorAll('button,input,textarea,select,a[href],[tabindex]')].filter(el=>!el.disabled&&el.tabIndex>=0&&!el.hidden&&el.getClientRects().length);
      if(!candidates.length){e.preventDefault();close.focus();return;}
      const first=candidates[0],last=candidates[candidates.length-1];
      if(e.shiftKey&&(document.activeElement===first||!host.contains(document.activeElement))){e.preventDefault();last.focus();}
      else if(!e.shiftKey&&(document.activeElement===last||!host.contains(document.activeElement))){e.preventDefault();first.focus();}
    });
    function fitScreen(){
      const mon=lastState?.monitors?.find(m=>m.i===controls.monitor?.());
      const ratio=mon?.w&&mon?.h?mon.w/mon.h:(image.naturalWidth&&image.naturalHeight?image.naturalWidth/image.naturalHeight:16/9);
      const availableW=screen.clientWidth,availableH=screen.clientHeight;
      if(!availableW||!availableH)return;
      const width=Math.min(availableW,availableH*ratio);
      view.style.width=`${Math.max(1,width)}px`;view.style.height=`${Math.max(1,width/ratio)}px`;
    }
    function syncNotice(){
      const mirror=lastState?.settings?.mirror_enabled!==false&&$('show_mirror').checked;
      notice.hidden=mirror&&loaded;
      notice.querySelector('strong').textContent=!mirror?'画面ミラーはお休み中':'画面を待っています';
      notice.querySelector('span').textContent=!mirror?'ツールと設定から、画面を表示できます。':'接続したPCの画面が、ここに届きます。';
    }
    image.addEventListener('load',()=>{loaded=true;fitScreen();syncNotice();});
    image.addEventListener('error',()=>{if(!image.naturalWidth){loaded=false;syncNotice();}});
    $('show_mirror').addEventListener('change',syncNotice);
    $('seg_mon').addEventListener('click',()=>{requestAnimationFrame(()=>{fitScreen();syncCursorFromState(lastState);});});
    if(window.ResizeObserver)new ResizeObserver(fitScreen).observe(screen);
    document.addEventListener('fullscreenchange',()=>requestAnimationFrame(fitScreen));
    function viewport(){document.body.style.setProperty('--sh-vh',`${window.visualViewport?.height||window.innerHeight}px`);fitScreen();}
    window.addEventListener('resize',viewport);window.visualViewport?.addEventListener('resize',viewport);viewport();
    let predicted={m:controls.monitor?.()||0,x:.5,y:.5},predictionTime=0,moveFrame=0,held=false;
    let padGesture=null,padHoldTimer=null,padTap=null,padTapTimer=null;
    const pointers=new Map();
    const enabled=()=>controls.state?.()?.settings?.control_enabled!==false;
    function submit(event){
      if(!enabled()&&event.type!=='up'){padStatus.textContent='PC側でリモート操作が無効になっています。';return Promise.resolve(false);}
      try{return Promise.resolve(controls.input(event)).catch(()=>false);}catch{return Promise.resolve(false);}
    }
    function paintCursor(){cursor.style.left=`${predicted.x*100}%`;cursor.style.top=`${predicted.y*100}%`;cursor.hidden=panel!=='pointer';cursor.classList.toggle('sh-cursor-held',held);}
    function syncCursorFromState(st){
      const monitor=controls.monitor?.()||0;
      if(predicted.m!==monitor)predicted={m:monitor,x:.5,y:.5};
      if(st?.cursor?.m===monitor&&Number.isFinite(st.cursor.x)&&Number.isFinite(st.cursor.y)&&!pointers.size&&!held&&Date.now()-predictionTime>250){predicted={m:monitor,x:Math.min(1,Math.max(0,st.cursor.x)),y:Math.min(1,Math.max(0,st.cursor.y))};}
      paintCursor();
    }
    function flushMove(){if(moveFrame){cancelAnimationFrame(moveFrame);moveFrame=0;return submit({type:'move',...predicted});}return Promise.resolve(true);}
    function scheduleMove(){predictionTime=Date.now();paintCursor();if(!moveFrame)moveFrame=requestAnimationFrame(()=>{moveFrame=0;submit({type:'move',...predicted});});}
    function flushPadTap(){
      if(!padTap)return Promise.resolve(true);
      clearTimeout(padTapTimer);const tap=padTap;padTap=null;return submit({type:'click',m:tap.m,x:tap.x,y:tap.y});
    }
    function sendRemote(event){flushPadTap();flushMove();return submit(event);}
    function padClick(){
      flushMove();
      navigator.vibrate?.(4);
      if(padTap&&Date.now()-padTap.time<350&&padTap.m===predicted.m&&Math.hypot(padTap.x-predicted.x,padTap.y-predicted.y)<.025){clearTimeout(padTapTimer);padTap=null;submit({type:'double_click',...predicted});}
      else{flushPadTap();padTap={...predicted,time:Date.now()};padTapTimer=setTimeout(flushPadTap,350);}
    }
    function releaseDrag(){
      if(!held)return;
      flushMove();held=false;drag.textContent='ドラッグ';drag.setAttribute('aria-pressed','false');pad.classList.remove('sh-pad-held');padStatus.textContent='マウスを離しました。';paintCursor();submit({type:'up',...predicted});
    }
    function resetPad(){clearTimeout(padHoldTimer);pointers.clear();padGesture=null;pad.classList.remove('sh-pad-active');}
    left.onclick=()=>sendRemote({type:'click',...predicted});right.onclick=()=>sendRemote({type:'click',button:'right',...predicted});
    drag.onclick=()=>{if(held){releaseDrag();return;}if(!enabled())return;flushPadTap();flushMove();held=true;drag.textContent='離す';drag.setAttribute('aria-pressed','true');pad.classList.add('sh-pad-held');padStatus.textContent='ドラッグ中です。もう一度押すと離します。';paintCursor();submit({type:'down',...predicted}).then(ok=>{if(!ok)releaseDrag();});};
    pad.addEventListener('pointerdown',e=>{
      if(!enabled())return;
      e.preventDefault();pad.setPointerCapture(e.pointerId);pointers.set(e.pointerId,{x:e.clientX,y:e.clientY});pad.classList.add('sh-pad-active');
      if(pointers.size>1){clearTimeout(padHoldTimer);if(padGesture)padGesture.multi=true;releaseDrag();return;}
      padGesture={id:e.pointerId,startX:e.clientX,startY:e.clientY,lastX:e.clientX,lastY:e.clientY,time:Date.now(),moved:false,multi:false,acted:false};
      padHoldTimer=setTimeout(()=>{if(padGesture&&!padGesture.moved&&!padGesture.multi&&!held){padGesture.acted=true;sendRemote({type:'click',button:'right',...predicted});}},560);
    });
    pad.addEventListener('pointermove',e=>{
      const previous=pointers.get(e.pointerId);if(!previous||!padGesture)return;e.preventDefault();
      pointers.set(e.pointerId,{x:e.clientX,y:e.clientY});
      if(pointers.size>=2){
        clearTimeout(padHoldTimer);padGesture.multi=true;const dy=(previous.y-e.clientY)/32/pointers.size;
        if(dy)sendRemote({type:'scroll',dy,...predicted});return;
      }
      if(padGesture.multi)return;
      const dx=e.clientX-padGesture.lastX,dy=e.clientY-padGesture.lastY;
      padGesture.lastX=e.clientX;padGesture.lastY=e.clientY;
      if(Math.hypot(e.clientX-padGesture.startX,e.clientY-padGesture.startY)>6){padGesture.moved=true;clearTimeout(padHoldTimer);}
      if(!dx&&!dy)return;
      flushPadTap();
      predicted.m=controls.monitor?.()||0;
      predicted.x=Math.min(1,Math.max(0,predicted.x+dx*1.5/(view.clientWidth||screen.clientWidth||1)));
      predicted.y=Math.min(1,Math.max(0,predicted.y+dy*1.5/(view.clientHeight||screen.clientHeight||1)));
      scheduleMove();
    });
    function endPad(e,cancelled){
      if(!pointers.has(e.pointerId))return;
      pointers.delete(e.pointerId);clearTimeout(padHoldTimer);flushMove();
      if(cancelled){releaseDrag();resetPad();return;}
      if(!pointers.size){if(padGesture&&!padGesture.moved&&!padGesture.multi&&!padGesture.acted&&!held&&Date.now()-padGesture.time<500)padClick();resetPad();}
    }
    pad.addEventListener('pointerup',e=>endPad(e,false));pad.addEventListener('pointercancel',e=>endPad(e,true));pad.addEventListener('lostpointercapture',e=>{if(pointers.has(e.pointerId))endPad(e,true);});pad.addEventListener('contextmenu',e=>e.preventDefault());
    // スクロールストリップのドラッグ（下に引くと下へ、上に引くと上へ）
    let stripId=null,stripLastY=0;
    strip.addEventListener('pointerdown',e=>{
      if(!enabled())return;
      e.preventDefault();e.stopPropagation();strip.setPointerCapture(e.pointerId);stripId=e.pointerId;stripLastY=e.clientY;strip.classList.add('on');navigator.vibrate?.(6);
    });
    strip.addEventListener('pointermove',e=>{
      if(e.pointerId!==stripId)return;e.preventDefault();
      const dy=(e.clientY-stripLastY)/24;stripLastY=e.clientY;
      if(dy)sendRemote({type:'scroll',dy,x:predicted.x,y:predicted.y,m:controls.monitor?.()||0});
    });
    const stripEnd=e=>{if(e.pointerId!==stripId)return;stripId=null;strip.classList.remove('on');};
    strip.addEventListener('pointerup',stripEnd);strip.addEventListener('pointercancel',stripEnd);strip.addEventListener('lostpointercapture',stripEnd);
    strip.addEventListener('keydown',e=>{
      if(!enabled())return;
      const dy={ArrowUp:-2,ArrowDown:2,PageUp:-8,PageDown:8}[e.key];
      if(dy){e.preventDefault();sendRemote({type:'scroll',dy,x:predicted.x,y:predicted.y,m:controls.monitor?.()||0});}
    });
    pad.addEventListener('keydown',e=>{
      const delta={ArrowLeft:[-.025,0],ArrowRight:[.025,0],ArrowUp:[0,-.025],ArrowDown:[0,.025]}[e.key];
      if(delta){e.preventDefault();if(!enabled())return;predicted.x=Math.min(1,Math.max(0,predicted.x+delta[0]));predicted.y=Math.min(1,Math.max(0,predicted.y+delta[1]));scheduleMove();}
      else if(e.key==='Enter'||e.key===' '){e.preventDefault();sendRemote({type:'click',...predicted});}
    });
    window.addEventListener('blur',()=>{flushPadTap();releaseDrag();resetPad();});
    document.addEventListener('visibilitychange',()=>{if(document.hidden){flushPadTap();releaseDrag();resetPad();}});
    function update(st){
      lastState=st||lastState;const allow=enabled();
      document.querySelectorAll('.sh-remote-action').forEach(b=>{b.disabled=!allow;});pad.setAttribute('aria-disabled',String(!allow));
      if(!allow){releaseDrag();resetPad();padStatus.textContent='PC側でリモート操作が無効になっています。';}
      else if(padStatus.textContent==='PC側でリモート操作が無効になっています。')padStatus.textContent='';
      syncCursorFromState(lastState);fitScreen();syncNotice();
    }
    window.addEventListener('shut:state',e=>update(e.detail));
    document.addEventListener('shut:state',e=>update(e.detail));
    const host=['localhost','127.0.0.1','[::1]','::1'].includes(location.hostname)&&new URLSearchParams(location.search).get('ephemeral')!=='1';
    if(host)openPanel('connect',connectQuick);else syncNav();
    loaded=image.complete&&image.naturalWidth>0;update(lastState);
    window.shutShell={open:openPanel,close:closePanel,panel:()=>panel,fit:fitScreen};
  };
  if(document.readyState==='loading')document.addEventListener('DOMContentLoaded',boot,{once:true});else boot();
})();
