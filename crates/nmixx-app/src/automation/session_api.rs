//! Language-neutral composition of the existing Application API. There is no CLI,
//! transport constructor, framing code, or independent parameter state here.
use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}, mpsc};
use std::time::{Duration, Instant};
use serde::Deserialize;
use serde_json::{Value, json};
use crate::{ActionHandle, ApplicationSession, AxdrStatus, IdentificationKind,
    IdentificationStart, MixedScopeConfig, MixedScopeSnapshot, ParameterValue,
    PositionValue, ScopeRate, ScopeSelection, SessionEvent, StreamState,
    TuningExperimentState};
use super::{AutomationApi, RunControl};

#[derive(Default)]
struct Owned { motor: bool, scope: bool, tuning: bool, identified: Option<&'static str> }
/// One instance corresponds to one Automation task, on its original connection.
/// Dropping a never-started runner releases the lease without touching the motor.
pub struct SessionWorkflowApi {
    app: ApplicationSession,
    cancelled: Arc<AtomicBool>,
    owned: Mutex<Owned>,
    finished: AtomicBool,
}
impl SessionWorkflowApi {
    pub fn new(app: ApplicationSession) -> Result<Self, String> {
        let (app, cancelled) = app.automation_claim().map_err(|e| e.to_string())?;
        Ok(Self { app, cancelled, owned: Mutex::new(Owned::default()), finished: AtomicBool::new(false) })
    }
    fn ids(&self, params: &Value) -> Result<Vec<u16>, String> {
        let keys = params.get("keys").and_then(Value::as_array).ok_or("keys must be an array")?;
        if keys.is_empty() || keys.len() > 256 { return Err("Read 1..256 keys per request".into()); }
        keys.iter().map(|key| self.id(key.as_str().ok_or("Each key must be a parameter symbol")?)).collect()
    }
    fn id(&self, key: &str) -> Result<u16, String> {
        self.app.schema().parameter_by_key(key).map(|m| m.id)
            .ok_or_else(|| format!("Parameter '{key}' is not exposed by this schema"))
    }
    fn read_state(&self) -> Result<u8, String> {
        match self.app.parameter_read(self.id("PARAM_MOTOR_STATE")?).map_err(|e| e.to_string())? {
            ParameterValue::U8(v) => Ok(v), _ => Err("Motor state has the wrong wire type".into()),
        }
    }
    fn claim_motion(&self, control: &RunControl) -> Result<(), String> {
        control.check()?;
        // Never make cleanup responsible for motion that already existed before
        // this script. The Application methods still perform their normal checks.
        if self.read_state()? == 2 { return Err("Motor is already RUN; stop it before starting another operation".into()); }
        control.check()?;
        // Set before the request: a transport error can leave acceptance uncertain.
        self.owned.lock().map_err(|_| "workflow lock poisoned")?.motor = true;
        Ok(())
    }
    fn row(&self, id: u16, result: Result<ParameterValue, String>) -> Value {
        let meta = self.app.schema().parameter_by_id(id);
        match result {
            Ok(value) => json!({"id":id, "symbol":meta.map(|m| &m.symbol), "value":typed(value), "error":null}),
            Err(error) => json!({"id":id, "symbol":meta.map(|m| &m.symbol), "value":null, "error":error}),
        }
    }
    fn selections(&self, params: &Value) -> Result<Vec<ScopeSelection>, String> {
        let channels = params.get("channels").and_then(Value::as_array).ok_or("channels must be an array")?;
        if channels.len() > 64 { return Err("Too many channel selections".into()); }
        channels.iter().map(|item| {
            let id = self.id(text(item, "key")?)?;
            let rate = match item.get("rate").and_then(Value::as_str).unwrap_or("normal") {
                "normal" => ScopeRate::Normal, "fast" => ScopeRate::Fast,
                _ => return Err("rate must be normal or fast".into()),
            };
            Ok(ScopeSelection { id, rate })
        }).collect()
    }
    fn scope_state(&self) -> Result<Value, String> {
        let config = self.app.scope_config().map_err(|e| e.to_string())?;
        let status = self.app.scope_status().map_err(|e| e.to_string())?;
        Ok(json!({"config":scope_config(&config), "state":stream_name(status.state),
            "samples":status.samples, "capacitySamples":status.capacity_samples, "lostFrames":status.lost_frames}))
    }
    fn wait_action(&self, events: &mpsc::Receiver<SessionEvent>, handle: ActionHandle,
        seconds: f64, control: &RunControl) -> Result<(), String> {
        let deadline = (Instant::now() + Duration::from_secs_f64(seconds)).min(control.deadline);
        loop {
            control.check()?;
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() { return Err("Action completion deadline exceeded".into()); }
            match events.recv_timeout(remaining.min(Duration::from_millis(20))) {
                Ok(SessionEvent::ActionCompleted { handle: done, status }) if done == handle => {
                    return if status == AxdrStatus::Ok { Ok(()) } else { Err(format!("Action failed: {status:?}")) };
                }
                Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {},
                Err(mpsc::RecvTimeoutError::Disconnected) => return Err("Original session closed while waiting for action".into()),
            }
        }
    }
    fn wait_stopped(&self, seconds: f64, control: &RunControl) -> Result<Value, String> {
        let deadline = (Instant::now() + Duration::from_secs_f64(seconds)).min(control.deadline);
        loop {
            control.check()?;
            let state = self.read_state()?;
            match state {
                0 | 1 => { self.owned.lock().map_err(|_| "workflow lock poisoned")?.motor = false; return Ok(json!({"state":state})); }
                2 => {}, _ => return Err(format!("Unknown motor state {state}")),
            }
            if Instant::now() >= deadline { return Err("Motor is still RUN after controlled-stop timeout".into()); }
            control.wait(Duration::from_millis(50))?;
        }
    }
    fn tuning_status(&self) -> Result<Value, String> {
        let status = self.app.tuning_experiment_status().map_err(|e| e.to_string())?;
        Ok(json!({"state":format!("{:?}",status.state).to_uppercase(),"message":status.message}))
    }
}
impl AutomationApi for SessionWorkflowApi {
    fn cancellation(&self) -> Arc<AtomicBool> { self.cancelled.clone() }
    fn call(&self, method: &str, params: Value, control: &RunControl) -> Result<Value, String> {
        control.check()?;
        if self.finished.load(Ordering::Acquire) { return Err("Automation task has already finished".into()); }
        if self.app.is_closing() { return Err("The original device session is closing".into()); }
        match method {
            "session.info" => Ok(json!({"apiVersion":1,"transportOwnership":"existing ApplicationSession",
                "schemaSource":self.app.schema().source.git_sha})),
            "parameter.list" => Ok(Value::Array(self.app.parameter_metadata().iter().map(|m| json!({
                "id":m.id,"symbol":m.symbol,"label":m.label,"typeName":m.type_name,"access":m.access,
                "unit":m.unit,"allowed":m.allowed.iter().map(|v| match v { crate::SchemaNumber::Integer(v) => json!(v), crate::SchemaNumber::Float(v) => json!(v) }).collect::<Vec<_>>(),
                "allowedSymbols":m.allowed_symbols})).collect())),
            "parameter.cached" | "parameter.read" => {
                let ids = self.ids(&params)?;
                let rows = if method == "parameter.read" { self.app.parameter_read_many(&ids) }
                    else { self.app.parameter_cached_many(&ids) }.map_err(|e| e.to_string())?;
                Ok(Value::Array(rows.into_iter().map(|(id, v)| self.row(id, v.map_err(|e| e.to_string()))).collect()))
            }
            "parameter.set" => {
                let id = self.id(text(&params,"key")?)?;
                let value: WireValue = serde_json::from_value(params.get("value").cloned().ok_or("value is required")?).map_err(|e| e.to_string())?;
                let value = self.app.parameter_write(id, value.into()).map_err(|e| e.to_string())?;
                Ok(self.row(id, Ok(value)))
            }
            "parameter.refresh" => {
                let rows = self.app.parameter_refresh_all().map_err(|e| e.to_string())?;
                Ok(Value::Array(rows.into_iter().map(|(id,v)| self.row(id,v.map_err(|e|e.to_string()))).collect()))
            }
            "motor.enable" => Ok(accepted(self.app.motor_enable().map_err(|e|e.to_string())?, true)),
            "motor.disable" => {
                let h = self.app.motor_disable().map_err(|e|e.to_string())?;
                self.owned.lock().map_err(|_|"workflow lock poisoned")?.motor = false;
                Ok(accepted(h,true))
            }
            "motor.stop" => Ok(accepted(self.app.motor_stop().map_err(|e|e.to_string())?, false)),
            "motor.wait_stopped" => self.wait_stopped(seconds(&params,"timeoutSeconds",60.,0.05,60.)?,control),
            "motion.get" => serde_json::to_value(self.app.motion_get()).map_err(|e|e.to_string()),
            "motion.set" => {
                let config = serde_json::from_value(params.get("config").cloned().ok_or("config is required")?).map_err(|e|e.to_string())?;
                serde_json::to_value(self.app.motion_set(config).map_err(|e|e.to_string())?).map_err(|e|e.to_string())
            }
            "motion.run" => {
                self.claim_motion(control)?;
                Ok(accepted(self.app.motion_run().map_err(|e|e.to_string())?, false))
            }
            "identification.run" => {
                let (kind,label) = match text(&params,"kind")? {
                    "rs_ls" => (IdentificationKind::RsLs,"rs_ls"), "flux" => (IdentificationKind::Flux,"flux"),
                    "jb" => (IdentificationKind::Jb,"jb"), _ => return Err("kind must be rs_ls, flux or jb".into()),
                };
                let timeout = seconds(&params,"timeoutSeconds",60.,0.05,300.)?;
                let allow_enable = params.get("allowEnable").and_then(Value::as_bool).unwrap_or(false);
                let events = self.app.subscribe().map_err(|e|e.to_string())?;
                self.claim_motion(control)?;
                self.owned.lock().map_err(|_|"workflow lock poisoned")?.identified = None;
                match self.app.identification_start(kind, allow_enable).map_err(|e|e.to_string())? {
                    IdentificationStart::Started(handle) => {
                        self.wait_action(&events,handle,timeout,control)?;
                        let mut owned = self.owned.lock().map_err(|_|"workflow lock poisoned")?;
                        owned.motor = false; owned.identified = Some(label);
                        Ok(json!({"kind":label,"completed":true,"txn":handle.txn.get(),"actionId":handle.action_id}))
                    }
                    IdentificationStart::RequiresEnable => {
                        self.owned.lock().map_err(|_|"workflow lock poisoned")?.motor = false;
                        Err("Enable the motor explicitly or pass allowEnable=true for this identification operation".into())
                    }
                    IdentificationStart::Blocked(issues) => {
                        self.owned.lock().map_err(|_|"workflow lock poisoned")?.motor = false;
                        Err(format!("Identification preflight blocked: {issues:?}"))
                    }
                }
            }
            "identification.apply" => {
                let kind = text(&params,"kind")?;
                if self.owned.lock().map_err(|_|"workflow lock poisoned")?.identified != Some(kind) {
                    return Err("Apply requires a matching successful identification from this workflow".into());
                }
                let handle = self.app.identification_apply().map_err(|e|e.to_string())?;
                self.owned.lock().map_err(|_|"workflow lock poisoned")?.identified = None;
                Ok(accepted(handle,true))
            }
            "encoder.phase_search" => {
                let timeout = seconds(&params,"timeoutSeconds",60.,0.05,300.)?;
                let events = self.app.subscribe().map_err(|e|e.to_string())?;
                self.claim_motion(control)?;
                let handle = self.app.phase_search_start().map_err(|e|e.to_string())?;
                self.wait_action(&events,handle,timeout,control)?;
                self.owned.lock().map_err(|_|"workflow lock poisoned")?.motor = false;
                Ok(accepted(handle,true))
            }
            "config.save" => {
                let handle = self.app.config_save().map_err(|e|e.to_string())?;
                // Capture the exact successful post-save baseline before the next
                // script write. A delayed UI must not mark later RAM writes saved.
                let rows = self.app.parameter_refresh_all().map_err(|e|e.to_string())?;
                let mut values = Vec::new();
                for (id,result) in rows { values.push(self.row(id,Ok(result.map_err(|e|e.to_string())?))); }
                Ok(json!({"completed":true,"txn":handle.txn.get(),"savedParameters":values}))
            }
            "scope.configure" => {
                let channels = self.selections(&params)?;
                let history = seconds(&params,"historySeconds",10.,0.001,60.)?;
                self.app.scope_configure(&channels,Duration::from_secs_f64(history),1).map_err(|e|e.to_string())?;
                self.scope_state()
            }
            "scope.live" | "scope.capture" => {
                let was_active = matches!(self.app.scope_status().map_err(|e|e.to_string())?.state,StreamState::Live|StreamState::Capturing);
                if method == "scope.capture" {
                    if was_active { return Err("Stop the existing Scope record explicitly before starting a finite capture".into()); }
                    let duration = seconds(&params,"seconds",1.,0.001,60.)?;
                    self.owned.lock().map_err(|_|"workflow lock poisoned")?.scope = true;
                    self.app.scope_capture(Duration::from_secs_f64(duration)).map_err(|e|e.to_string())?;
                } else {
                    if !was_active { self.owned.lock().map_err(|_|"workflow lock poisoned")?.scope = true; }
                    self.app.scope_live().map_err(|e|e.to_string())?;
                }
                self.scope_state()
            }
            "scope.stop" => {
                self.app.scope_stop().map_err(|e|e.to_string())?;
                self.owned.lock().map_err(|_|"workflow lock poisoned")?.scope = false;
                self.scope_state()
            }
            "scope.clear" => { self.app.scope_clear().map_err(|e|e.to_string())?; self.scope_state() }
            "scope.status" => self.scope_state(),
            "scope.snapshot" | "scope.summary" => {
                let window = seconds(&params,"windowSeconds",0.5,0.0005,10.)?;
                let offset = seconds(&params,"endOffsetSeconds",0.,0.,60.)?;
                let data = self.app.scope_snapshot_window(Duration::from_secs_f64(window),Duration::from_secs_f64(offset)).map_err(|e|e.to_string())?;
                Ok(snapshot(data, method == "scope.summary"))
            }
            "tuning.start" => {
                let channels = if params.get("channels").is_some() { self.selections(&params)? }
                    else { self.app.tuning_experiment_default_selections().map_err(|e|e.to_string())? };
                self.claim_motion(control)?;
                self.owned.lock().map_err(|_|"workflow lock poisoned")?.tuning = true;
                self.app.tuning_experiment_start(&channels).map_err(|e|e.to_string())?;
                self.tuning_status()
            }
            "tuning.status" => self.tuning_status(),
            "tuning.stop" => { self.app.tuning_experiment_stop().map_err(|e|e.to_string())?; self.tuning_status() }
            "tuning.wait" => {
                let timeout = seconds(&params,"timeoutSeconds",60.,0.05,3600.)?;
                let deadline = (Instant::now()+Duration::from_secs_f64(timeout)).min(control.deadline);
                loop {
                    control.check()?;
                    let state = self.app.tuning_experiment_status().map_err(|e|e.to_string())?;
                    match state.state {
                        TuningExperimentState::Completed => {
                            let mut owned = self.owned.lock().map_err(|_|"workflow lock poisoned")?;
                            owned.motor = false; owned.tuning = false; return Ok(json!({"completed":true,"message":state.message}));
                        }
                        TuningExperimentState::Failed => return Err(state.message.unwrap_or("Tuning experiment failed".into())),
                        TuningExperimentState::Idle => return Err("No tuning experiment is running".into()),
                        _ => {}
                    }
                    if Instant::now() >= deadline { return Err("Tuning wait deadline exceeded".into()); }
                    control.wait(Duration::from_millis(50))?;
                }
            }
            "tuning.snapshot" => {
                let window = seconds(&params,"windowSeconds",3.,0.0005,10.)?;
                let offset = seconds(&params,"endOffsetSeconds",0.,0.,3600.)?;
                Ok(snapshot(self.app.tuning_record_window(Duration::from_secs_f64(window),Duration::from_secs_f64(offset))
                    .map_err(|e|e.to_string())?,false))
            }
            "stream.progress" => serde_json::to_value(self.app.runtime_stream_progress().map_err(|e|e.to_string())?).map_err(|e|e.to_string()),
            "wait" => { control.wait(Duration::from_secs_f64(seconds(&params,"seconds",0.,0.,3600.)?))?; Ok(Value::Null) }
            _ => Err(format!("Unknown Application operation '{method}'")),
        }
    }
    fn finish(&self) -> Result<(), String> {
        if self.finished.swap(true,Ordering::AcqRel) { return Ok(()); }
        let owned = self.owned.lock().map_err(|_|"workflow lock poisoned")?;
        let cleanup = self.app.automation_cleanup_handle();
        let mut errors = Vec::new();
        if owned.motor || owned.tuning {
            if let Err(e) = cleanup.motor_stop() { errors.push(format!("Motor Stop: {e}")); }
            if let Err(e) = cleanup.automation_wait_stopped() { errors.push(e.to_string()); }
        }
        if owned.tuning {
            if let Err(e) = cleanup.automation_join_tuning() { errors.push(e.to_string()); }
        }
        if owned.scope {
            if let Err(e) = cleanup.scope_stop() { errors.push(format!("Scope Stop: {e}")); }
        }
        self.app.automation_release();
        if errors.is_empty() { Ok(()) } else { Err(errors.join("; ")) }
    }
    fn final_effects(&self) -> Vec<(String, Value)> {
        if self.owned.lock().map(|owned| owned.scope).unwrap_or(false) {
            if let Ok(state) = self.scope_state() { return vec![("scope.changed".into(), state)]; }
        }
        Vec::new()
    }
}
impl Drop for SessionWorkflowApi {
    fn drop(&mut self) {
        // The runner normally reports finish() before dropping the adapter. This
        // fallback prevents a caller abandoning owned motion by dropping it directly.
        let _ = self.finish();
        self.app.automation_release();
    }
}
fn text<'a>(params:&'a Value,key:&str)->Result<&'a str,String> {
    params.get(key).and_then(Value::as_str).ok_or_else(||format!("{key} must be text"))
}
fn seconds(params:&Value,key:&str,default:f64,min:f64,max:f64)->Result<f64,String> {
    let value = match params.get(key) { None=>default,Some(v)=>v.as_f64().ok_or_else(||format!("{key} must be numeric"))? };
    if value.is_finite() && (min..=max).contains(&value) { Ok(value) } else { Err(format!("{key} must be within {min}..{max}")) }
}
fn accepted(handle:ActionHandle,completed:bool)->Value { json!({"txn":handle.txn.get(),"actionId":handle.action_id,"accepted":true,"completed":completed}) }
fn stream_name(state:StreamState)->&'static str { match state { StreamState::Live=>"LIVE",StreamState::Capturing=>"CAPTURING",StreamState::Paused=>"PAUSED",StreamState::Stopped=>"STOPPED" } }
fn scope_config(config:&MixedScopeConfig)->Value {
    json!({"historySeconds":config.history.as_secs_f64(),"channels":config.channels.iter().map(|m|json!({
        "id":m.id,"label":m.label,"unit":m.unit,"rate":if m.rate==ScopeRate::Fast {"fast"} else {"normal"},"sampleRateHz":m.sample_rate_hz})).collect::<Vec<_>>()})
}
fn snapshot(data:MixedScopeSnapshot,summary:bool)->Value {
    json!({"state":stream_name(data.state),"lostFrames":data.lost_frames,"series":data.series.into_iter().map(|s| {
        let mut row=json!({"id":s.id,"sampleRateHz":s.sample_rate_hz,"sampleCount":s.values.len(),"first":s.values.first(),"last":s.values.last(),
            "min":s.values.iter().copied().filter(|v|v.is_finite()).reduce(f32::min),"max":s.values.iter().copied().filter(|v|v.is_finite()).reduce(f32::max)});
        if !summary { row["values"]=json!(s.values); } row
    }).collect::<Vec<_>>()})
}
#[derive(Deserialize)]
#[serde(tag="type",content="value",rename_all="snake_case")]
enum WireValue { U8(u8),I8(i8),F32(f32),I32(i32),U32(u32),Position(WirePosition) }
#[derive(Deserialize)]
struct WirePosition { turns:i32,theta:f32 }
impl From<WireValue> for ParameterValue {
    fn from(v:WireValue)->Self { match v { WireValue::U8(v)=>Self::U8(v),WireValue::I8(v)=>Self::I8(v),WireValue::F32(v)=>Self::F32(v),
        WireValue::I32(v)=>Self::I32(v),WireValue::U32(v)=>Self::U32(v),WireValue::Position(v)=>Self::Position(PositionValue{turns:v.turns,theta:v.theta}) } }
}
fn typed(value:ParameterValue)->Value { match value {
    ParameterValue::U8(v)=>json!({"type":"u8","value":v}),ParameterValue::I8(v)=>json!({"type":"i8","value":v}),
    ParameterValue::F32(v)=>json!({"type":"f32","value":v}),ParameterValue::I32(v)=>json!({"type":"i32","value":v}),
    ParameterValue::U32(v)=>json!({"type":"u32","value":v}),ParameterValue::Position(v)=>json!({"type":"position","value":{"turns":v.turns,"theta":v.theta}})
} }
