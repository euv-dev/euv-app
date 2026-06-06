include!(concat!(env!("OUT_DIR"), "/config_generated.rs"));
include!(concat!(env!("OUT_DIR"), "/bundled_cache_data.rs"));

/// The filename used for the active version pointer symlink.
pub(crate) const ACTIVE_LINK: &str = "active";

/// The prefix for version directory names.
pub(crate) const VERSION_PREFIX: &str = "v_";

/// HTTP client timeout in seconds for fetch operations.
pub(crate) const FETCH_TIMEOUT_SECS: u64 = 30;

/// Maximum allowed response body size in bytes (10 MiB).
pub(crate) const MAX_BODY_SIZE: usize = 10485760;

/// Interval in milliseconds between fetch retry attempts.
pub(crate) const RETRY_INTERVAL_MILLIS: u64 = 1000;

/// The custom URI scheme name used for serving cached resources.
pub(crate) const SCHEME_NAME: &str = "euv";

/// Maximum number of old version directories to keep.
pub(crate) const MAX_KEPT_VERSIONS: usize = 2;

/// JavaScript snippet injected into index.html to listen for reload events from the Tauri backend.
pub(crate) const RELOAD_LISTENER_SCRIPT: &str = r#"<script>
(function(){
  if(window.__TAURI__&&window.__TAURI__.event){
    window.__TAURI__.event.listen('euv://reload',function(){
      window.location.reload();
    });
  } else {
    document.addEventListener('DOMContentLoaded',function(){
      var t=setInterval(function(){
        if(window.__TAURI__&&window.__TAURI__.event){
          clearInterval(t);
          window.__TAURI__.event.listen('euv://reload',function(){
            window.location.reload();
          });
        }
      },100);
      setTimeout(function(){clearInterval(t);},10000);
    });
  }
})();
</script>"#;

/// Debug panel HTML snippet injected into index.html in debug builds.
///
/// Displays cache source information and a log viewer for real-time debugging.
#[cfg(debug_assertions)]
pub(crate) const DEBUG_PANEL_SCRIPT: &str = r#"<script>
(function(){
  var expanded=false;
  var bar=document.createElement('div');
  bar.id='__euv_debug_bar';
  bar.style.cssText='position:fixed;bottom:0;left:0;right:0;height:36px;background:#1a1a2e;color:#0f0;font:13px/36px monospace;padding:0 12px;z-index:2147483647;cursor:pointer;user-select:none;display:flex;align-items:center;box-shadow:0 -2px 8px rgba(0,0,0,0.5);';
  bar.textContent='\u25B2 [DEBUG] source: {{SOURCE}} | tap to expand';

  var panel=document.createElement('div');
  panel.id='__euv_debug_panel';
  panel.style.cssText='position:fixed;bottom:0;left:0;right:0;height:50vh;background:#0d0d1a;color:#0f0;font:12px monospace;z-index:2147483646;display:none;flex-direction:column;box-shadow:0 -4px 16px rgba(0,0,0,0.7);';

  var header=document.createElement('div');
  header.style.cssText='padding:8px 12px;background:#1a1a2e;border-bottom:1px solid #333;flex-shrink:0;display:flex;justify-content:space-between;align-items:center;';
  header.innerHTML='<span style="color:#0f0;font-weight:bold;">EUV Debug Console</span><span id="__euv_close" style="color:#f55;cursor:pointer;font-size:18px;">\u2716</span>';

  var info=document.createElement('div');
  info.style.cssText='padding:6px 12px;background:#111;border-bottom:1px solid #222;flex-shrink:0;color:#aaa;font-size:11px;word-break:break-all;';
  info.textContent='source: {{SOURCE}} | path: {{PATH}}';

  var logArea=document.createElement('div');
  logArea.id='__euv_debug_logs';
  logArea.style.cssText='flex:1;overflow-y:auto;padding:8px 12px;';

  panel.appendChild(header);
  panel.appendChild(info);
  panel.appendChild(logArea);

  function toggle(){
    expanded=!expanded;
    if(expanded){
      panel.style.display='flex';
      bar.style.bottom='50vh';
      bar.textContent='\u25BC [DEBUG] source: {{SOURCE}} | tap to collapse';
    } else {
      panel.style.display='none';
      bar.style.bottom='0';
      bar.textContent='\u25B2 [DEBUG] source: {{SOURCE}} | tap to expand';
    }
  }
  bar.addEventListener('click',toggle);

  function addLog(msg){
    var line=document.createElement('div');
    line.style.cssText='padding:2px 0;border-bottom:1px solid #1a1a2e;color:#0f0;word-break:break-all;';
    var now=new Date();
    var ts=now.getHours().toString().padStart(2,'0')+':'+now.getMinutes().toString().padStart(2,'0')+':'+now.getSeconds().toString().padStart(2,'0')+'.'+now.getMilliseconds().toString().padStart(3,'0');
    line.textContent='['+ts+'] '+msg;
    logArea.appendChild(line);
    logArea.scrollTop=logArea.scrollHeight;
  }

  function initListener(){
    if(window.__TAURI__&&window.__TAURI__.event){
      window.__TAURI__.event.listen('euv://debug-log',function(e){
        addLog(e.payload||'');
      });
      addLog('[panel] listener registered');
      addLog('[panel] source: {{SOURCE}}');
      addLog('[panel] path: {{PATH}}');
    } else {
      var t=setInterval(function(){
        if(window.__TAURI__&&window.__TAURI__.event){
          clearInterval(t);
          window.__TAURI__.event.listen('euv://debug-log',function(e){
            addLog(e.payload||'');
          });
          addLog('[panel] listener registered');
          addLog('[panel] source: {{SOURCE}}');
          addLog('[panel] path: {{PATH}}');
        }
      },100);
      setTimeout(function(){clearInterval(t);},10000);
    }
  }

  document.addEventListener('DOMContentLoaded',function(){
    document.body.appendChild(panel);
    document.body.appendChild(bar);
    document.getElementById('__euv_close').addEventListener('click',function(e){
      e.stopPropagation();
      toggle();
    });
    initListener();
  });
})();
</script>"#;
