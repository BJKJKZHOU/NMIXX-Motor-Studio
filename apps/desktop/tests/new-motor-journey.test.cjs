// Real Svelte page command handlers, connected by one scripted device/session.
// Separately, Rust preflight tests call the real ParameterService against a fake
// FrameTransport. This test does not render a WebView or energize hardware.
const { test } = require("node:test");
const assert = require("node:assert/strict");
const { readFileSync } = require("node:fs");
const { join } = require("node:path");
const vm = require("node:vm");
const ts = require("typescript");
const root = join(__dirname, "../src");
const source = page => readFileSync(join(root,page),"utf8");
const transpile = code => ts.transpileModule(code,{
  compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS},
}).outputText;
function extract(src,name) {
  const prefix=src.match(new RegExp(`^  (?:async )?function ${name}\\([^\\n]*?\\)[^\\n]*`,"m"));
  assert.ok(prefix,`Missing actual ${name} handler`);
  if(prefix[0].trimEnd().endsWith("}")) return prefix[0];
  const full=src.match(new RegExp(`^  (?:async )?function ${name}\\([^\\n]*?\\)[^\\n]*\\{[\\s\\S]*?^  \\}`,"m"));
  assert.ok(full,`Missing complete ${name} handler`);
  return full[0];
}
function handlers(file,names,fields,extra="") {
  const sandbox=vm.createContext(fields);
  const code=extra+"\n"+names.map(name=>extract(source(file),name)).join("\n");
  vm.runInContext(transpile(code),sandbox);
  return {ctx:sandbox,get:expression=>vm.runInContext(expression,sandbox)};
}
function deviceFixture() {
  const device={
    motorState:"DISABLED",mode:"TORQUE",encoderReady:false,calValid:false,
    candidate:null,txn:1,commands:[],experiment:"IDLE",
    values:{
      PARAM_LIMIT_I_MAX:{type:"f32",value:2},PARAM_LIMIT_WM_MAX:{type:"f32",value:40},
      PARAM_MOTOR_RL_I_PEAK:{type:"f32",value:2},
      PARAM_MOTOR_ALIGN_CURRENT:{type:"f32",value:0},
      PARAM_MOTOR_IF_CURRENT:{type:"f32",value:0},
      PARAM_MOTOR_RS:{type:"f32",value:0},PARAM_MOTOR_LD:{type:"f32",value:0},
      PARAM_MOTOR_LQ:{type:"f32",value:0},PARAM_MOTOR_FLUX:{type:"f32",value:0},
      PARAM_MOTOR_J:{type:"f32",value:0},PARAM_MOTOR_B:{type:"f32",value:0},
      PARAM_IDENT_RS_LS_VALID:{type:"u8",value:0},
      PARAM_IDENT_FLUX_VALID:{type:"u8",value:0},
      PARAM_IDENT_JB_VALID:{type:"u8",value:0},
      PARAM_MOTOR_PP:{type:"u8",value:7},
      PARAM_IDENT_JB_EXCITE_RATIO:{type:"f32",value:0.2},
      PARAM_IDENT_JB_EXCITE_HZ:{type:"f32",value:3},
    },
  };
  device.write=(key,value)=>{
    if(device.motorState!=="DISABLED" && (
      key.startsWith("PARAM_MOTOR_") || key.startsWith("PARAM_LIMIT_") ||
      key.startsWith("PARAM_ENCODER_"))) throw Error("ERR_STATE");
    device.commands.push(["PARAM_WRITE",key,value.value]);
    device.values[key]=value;
    if(key==="PARAM_ENCODER_PROTOCOL") device.encoderReady=value.value===1;
  };
  device.start=async(key,allowEnable)=>{
    device.commands.push(["identification_start",key,allowEnable]);
    const value=k=>device.values[k]?.value??0;
    let issue=null;
    if(key==="rsLs" && value("PARAM_MOTOR_RL_I_PEAK")>=value("PARAM_LIMIT_I_MAX"))
      issue="Rs/Ls injection peak must be below current limit";
    if(key!=="rsLs" && (value("PARAM_MOTOR_ALIGN_CURRENT")<=0 ||
      value("PARAM_MOTOR_IF_CURRENT")<=0)) issue="Align / I/F current must be configured";
    if(key==="flux" && value("PARAM_MOTOR_RS")<=0)issue="Apply Rs/Ls first";
    if(key==="jb" && value("PARAM_MOTOR_FLUX")<=0)issue="Apply Flux first";
    if(issue)return{status:"blocked",issues:[{reason:issue}]};
    if(device.motorState==="DISABLED"&&!allowEnable)return{status:"requires_enable"};
    if(device.motorState==="RUN")throw Error("Motor already running");
    if(device.mode!=="IDENT" || device.motorState==="DISABLED"){
      if(device.motorState==="ENABLED"){
        device.commands.push(["ACTION","ACTION_MOTOR_DISABLE"]);
        device.motorState="DISABLED";
      }
      device.commands.push(["PARAM_WRITE","PARAM_MOTOR_MODE","IDENT"]);
      device.mode="IDENT";
      device.commands.push(["ACTION","ACTION_MOTOR_ENABLE"]);
      device.motorState="ENABLED";
    }
    const action={rsLs:"ACTION_IDENT_RS_LS_START",flux:"ACTION_IDENT_FLUX_START",jb:"ACTION_IDENT_JB_START"}[key];
    device.commands.push(["ACTION",action]);
    device.motorState="RUN";
    device.last={key,handle:{txn:device.txn++,actionId:0x1100+device.txn}};
    return{status:"started",handle:device.last.handle};
  };
  device.complete=key=>{
    assert.equal(device.last.key,key);
    device.motorState="ENABLED";device.candidate=key;
    device.values[{rsLs:"PARAM_IDENT_RS_LS_VALID",flux:"PARAM_IDENT_FLUX_VALID",jb:"PARAM_IDENT_JB_VALID"}[key]]={type:"u8",value:1};
    if(key==="rsLs"){
      device.values.PARAM_IDENT_RS_RESULT={type:"f32",value:0.38};
      device.values.PARAM_IDENT_LS_RESULT={type:"f32",value:0.00015};
    }else if(key==="flux")device.values.PARAM_IDENT_FLUX_RESULT={type:"f32",value:0.007};
    else{
      device.values.PARAM_IDENT_J_RESULT={type:"f32",value:0.00003};
      device.values.PARAM_IDENT_B_RESULT={type:"f32",value:0.000001};
    }
    return{...device.last.handle,operation:{rsLs:"identification.rsLs",flux:"identification.flux",jb:"identification.jb"}[key],status:"Ok",ok:true};
  };
  device.apply=async key=>{
    assert.equal(device.candidate,key);
    device.commands.push(["ACTION","ACTION_IDENT_APPLY",key]);
    const pairs=key==="rsLs"?[
      ["PARAM_MOTOR_RS","PARAM_IDENT_RS_RESULT"],["PARAM_MOTOR_LD","PARAM_IDENT_LS_RESULT"],["PARAM_MOTOR_LQ","PARAM_IDENT_LS_RESULT"],
    ]:key==="flux"?[["PARAM_MOTOR_FLUX","PARAM_IDENT_FLUX_RESULT"]]:[
      ["PARAM_MOTOR_J","PARAM_IDENT_J_RESULT"],["PARAM_MOTOR_B","PARAM_IDENT_B_RESULT"],
    ];
    for(const [active,result] of pairs)device.values[active]=device.values[result];
    device.candidate=null;
    return{txn:device.txn++,actionId:0x1104};
  };
  return device;
}

test("new motor from USB connection through three identifications, phase search and tuning",async()=>{
  const device=deviceFixture(),errors=[];
  const connection={port:"COM7",commissioning:{
    identificationRsLs:true,identificationFlux:true,identificationJb:true,
    identificationApply:true,phaseSearch:true,
  },motion:{run:true,stop:true,position:true,speed:true,torque:true},
    fastMaxChannels:8,normalMaxChannels:8,
    channels:[{id:17,label:"Iq",supportsFast:true,supportsNormal:true}]};
  let connected=false;
  const conn=handlers("connection/ConnectionPage.svelte",["connect"],{
    port:"COM7",schemaPath:"axdr-host-schema.toml",busy:false,connection:undefined,
    onSchemaPathChanged:()=>{},onConnected:info=>{connected=info.port==="COM7";},
    onError:error=>errors.push(String(error)),
    connectDevice:async(port,_,baud)=>{
      device.commands.push(["CONNECT",port,baud]);return connection;
    },
  });
  await conn.get("connect()");
  assert.equal(connected,true);
  assert.deepEqual(device.commands[0],["CONNECT","COM7",115200]);

  const permissions={exports:{}};
  vm.runInNewContext(transpile(source("parameters/writeState.ts")),{
    exports:permissions.exports,require:()=>({}),
  });
  const canWrite=permissions.exports.canWriteInMotorState;
  const commonMeta={access:"rw",writeState:"disabled"};
  const motorSource=source("motor/MotorPage.svelte");
  const config=motorSource.match(/^  const IDENT_CONFIGS: Record<IdentKey, IdentConfig> = \{[\s\S]*?^  \};/m);
  assert.ok(config);
  const motor=handlers("motor/MotorPage.svelte",[
    "handleKey","numericValue","identificationAvailable","setIdentState","identifyBusy",
    "isWritable","commit","startIdentification","applyIdentification","handleActionCompleted",
  ],{
    connection,motorState:device.motorState,
    $parameters:{loading:false,saving:false},metadata:Object.fromEntries(Object.keys(device.values).map(key=>[key,commonMeta])),
    values:device.values,writing:new Set(),generation:0,startingIdent:null,
    identStates:{rsLs:{phase:"idle",message:""},flux:{phase:"idle",message:""},jb:{phase:"idle",message:""}},
    pendingHandles:{},applyCandidate:null,earlyCompletion:undefined,
    failReasonSymbol:"PARAM_IDENT_FAIL_REASON",
    canWriteInMotorState:canWrite,
    edits:{commit:async key=>device.write(key,device.values[key])},
    startIdentificationAction:device.start,applyIdentificationAction:device.apply,
    onError:error=>errors.push(String(error)),
    window:{confirm:()=>true},
    $uiLanguage:"zh-CN",
    translateFormat:(key, language, values)=>{
      assert.equal(language,"zh-CN");
      if(key==="motor.identification.enable.confirm")return values.kind+" 辨识需要使能电机。";
      if(key==="motor.identification.failure.reason")return "原因代码 "+values.code;
      throw Error("unexpected translation key "+key);
    },
  },config[0]);

  // Inherited RL injection setting is unsafe for the chosen current limit.
  await motor.get('startIdentification("rsLs")');
  assert.equal(motor.get("identStates.rsLs.phase"),"failed");
  assert.match(motor.get("identStates.rsLs.message"),/injection peak/);
  assert.equal(device.commands.some(c=>c[1]==="ACTION_IDENT_RS_LS_START"),false);

  device.write("PARAM_MOTOR_RL_I_PEAK",{type:"f32",value:0.5});
  async function identify(key) {
    await motor.get(`startIdentification("${key}")`);
    assert.equal(motor.get(`identStates.${key}.phase`),"running");
    motor.ctx.completed=device.complete(key);
    motor.get("handleActionCompleted(completed)");
    assert.equal(motor.get(`identStates.${key}.phase`),"ready");
    await motor.get(`applyIdentification("${key}")`);
    assert.equal(motor.get(`identStates.${key}.phase`),"applied");
    assert.equal(device.motorState,"ENABLED");
  }
  await identify("rsLs");
  assert.equal(device.values.PARAM_MOTOR_RS.value,0.38);

  // Flux is now blocked by the missing Align and I/F setup.
  await motor.get('startIdentification("flux")');
  assert.equal(motor.get("identStates.flux.phase"),"failed");
  assert.match(motor.get("identStates.flux.message"),/Align/);
  motor.ctx.motorState=device.motorState;
  assert.equal(motor.get('isWritable("PARAM_MOTOR_ALIGN_CURRENT")'),false);
  
  const global=handlers("App.svelte",["toggleMotorEnable"],{
    connectionReady:true,motorState:device.motorState,globalActionBusy:false,
    motorActionNotice:"",onError:error=>errors.push(String(error)),
    disableMotor:async()=>{
      device.commands.push(["ACTION","ACTION_MOTOR_DISABLE"]);
      device.motorState="DISABLED";
    },
    enableMotor:async()=>{
      device.commands.push(["ACTION","ACTION_MOTOR_ENABLE"]);
      if(device.mode==="POSITION"&&!device.calValid)throw Error("Calibration required");
      device.motorState="ENABLED";
    },
  });
  await global.get("toggleMotorEnable()");
  assert.equal(device.motorState,"DISABLED");
  motor.ctx.motorState=device.motorState;
  assert.equal(motor.get('isWritable("PARAM_MOTOR_ALIGN_CURRENT")'),true);
  device.write("PARAM_MOTOR_ALIGN_CURRENT",{type:"f32",value:0.5});
  device.write("PARAM_MOTOR_IF_CURRENT",{type:"f32",value:0.5});
  await identify("flux");await identify("jb");
  assert.equal(device.values.PARAM_MOTOR_FLUX.value,0.007);
  assert.equal(device.values.PARAM_MOTOR_J.value,0.00003);

  // The next workflow is Encoder: protocol is DISABLED-only and a missing
  // encoder feedback produces failure before phase search may energize.
  const enc=handlers("encoder/EncoderPage.svelte",[
    "handleKey","isWritable","phaseSearchAvailable","setU8",
    "startPhaseSearch","handleActionCompleted",
  ],{
    connection,motorState:device.motorState,
    $parameters:{loading:false,saving:false},metadata:{
      PARAM_ENCODER_PROTOCOL:commonMeta,
    },writing:new Set(),phaseState:"idle",phaseMessage:"",
    pendingPhaseHandle:null,earlyPhaseCompletion:undefined,phaseRequestGeneration:0,
    homingState:"idle",homingMessage:"",pendingHomingHandle:null,earlyHomingCompletion:undefined,homingRequestGeneration:0,
    canWriteInMotorState:canWrite,
    edits:{select:async(key,value)=>device.write(key,value)},
    onError:error=>errors.push(String(error)),
    startPhaseSearchAction:async()=>{
      if(!device.encoderReady)throw Error("Encoder not ready");
      assert.equal(device.motorState,"DISABLED");
      device.commands.push(["PARAM_WRITE","PARAM_MOTOR_MODE","PHASE_SEARCH"]);
      device.mode="PHASE_SEARCH";
      device.commands.push(["ACTION","ACTION_MOTOR_ENABLE"]);
      device.motorState="ENABLED";
      device.commands.push(["ACTION","ACTION_MOTOR_RUN"]);
      device.motorState="RUN";
      return{txn:6,actionId:0x1002};
    },
  });
  await enc.get("startPhaseSearch()");
  assert.equal(enc.get("phaseState"),"failed");
  assert.match(enc.get("phaseMessage"),/Encoder not ready/);
  global.ctx.motorState=device.motorState;
  await global.get("toggleMotorEnable()");
  enc.ctx.motorState=device.motorState;
  assert.equal(device.motorState,"DISABLED");
  assert.equal(enc.get('isWritable("PARAM_ENCODER_PROTOCOL")'),true);
  await enc.get('setU8("PARAM_ENCODER_PROTOCOL",1)');
  assert.equal(device.encoderReady,true);
  await enc.get("startPhaseSearch()");
  assert.equal(enc.get("phaseState"),"running");
  enc.ctx.completed={txn:6,actionId:0x1002,operation:"encoder.phaseSearch",status:"Ok",ok:true};
  device.calValid=true;device.motorState="ENABLED";
  enc.get("handleActionCompleted(completed)");
  assert.equal(enc.get("phaseState"),"success");

  // Position mode must be selected while DISABLED; global enable then has
  // a calibrated encoder available and the Tuning page may start a motor run.
  // The newly identified model + phase calibration may only be persisted
  // through the global Save while the drive is DISABLED.
  const save=handlers("App.svelte",["savePersistentParameters"],{
    connectionReady:true,parameterSaveAvailable:true,motorState:device.motorState,
    globalActionBusy:false,saveFeedback:"idle",
    saveParameters:async()=>{},
    saveParameterBaseline:async()=>{
      device.commands.push(["ACTION","ACTION_CONFIG_SAVE"]);
    },
    showSavedFeedback:()=>{},
    onError:error=>errors.push(String(error)),
  });
  const beforeSave=device.commands.length;
  await save.get("savePersistentParameters()");
  assert.equal(device.commands.length,beforeSave);
  global.ctx.motorState=device.motorState;
  await global.get("toggleMotorEnable()");
  assert.equal(device.motorState,"DISABLED");
  save.ctx.motorState="DISABLED";
  await save.get("savePersistentParameters()");
  assert.deepEqual(device.commands.at(-1),["ACTION","ACTION_CONFIG_SAVE"]);
  const motion=handlers("motion/MotionPage.svelte",["setMode"],{
    motorState:"DISABLED",MOTION_MODE:"PARAM_MOTOR_MODE",
    metadata:{PARAM_MOTOR_MODE:commonMeta},
    modeParameterValue:(_,mode)=>mode==="position"?2:undefined,
    locked:()=>false,onError:error=>errors.push(String(error)),
    edits:{select:async(_,value)=>{
      device.write("PARAM_MOTOR_MODE",value);device.mode="POSITION";
    }},
  });
  await motion.get('setMode("position")');
  assert.equal(device.mode,"POSITION");
  device.commands.push(["PARAM_WRITE","PARAM_TARGET_SPEED",10]);
  global.ctx.motorState=device.motorState;
  await global.get("toggleMotorEnable()");
  assert.equal(device.motorState,"ENABLED");

  // Default Plot channel compatibility is intentionally excluded; the user
  // explicitly selects the supported Iq FAST channel before pressing Run.
  const tuning=handlers("control/ControlTuningPage.svelte",[
    "formatHostNumber","hasDirtyDraft","experimentActive","selectedTuningEntries",
    "selectedFastCount","selectedNormalCount","selectionWithinLimits",
    "toggleTuningChannel","canRunMotion","runTuningMotion","refreshExperiment",
  ],{
    connection,motorState:device.motorState,motionActionBusy:false,
    stopActionBusy:false,experimentState:"IDLE",experimentMessage:"",
    experimentSnapshot:undefined,experimentRefreshBusy:false,generation:0,
    waveformTab:"channels",tuningTimePerDiv:0.02,
    tuningSelections:{},displayMultipliers:{},writing:new Set(),
    $parameters:{loading:false,saving:false},$edits:{dirty:new Set()},
    $motionState:{incrementalDeltaTurn:1},incrementalDraft:"1",
    activeMotionMode:()=>device.mode==="POSITION"?"position":undefined,
    motionModeSupported:()=>true,waitMotionUpdates:async()=>{},
    startTuningExperiment:async selections=>{
      device.commands.push(["TUNING_CAPTURE",selections[0].id,selections[0].rate]);
      device.experiment="PREPARING";return{state:device.experiment,message:null};
    },
    tuningExperimentStatus:async()=>({state:device.experiment,message:null}),
    readTuningExperimentSnapshot:async()=>({
      recordedSeconds:0.5,windowSeconds:0.2,endOffsetSeconds:0,
      config:{channels:[]},snapshot:{series:[],lostFrames:0},
    }),
    onError:error=>errors.push(String(error)),
  });
  assert.equal(tuning.get("canRunMotion()"),false);
  tuning.get("toggleTuningChannel(17,true)");
  assert.equal(tuning.get("canRunMotion()"),true);
  await tuning.get("runTuningMotion()");
  assert.equal(tuning.get("experimentState"),"PREPARING");
  assert.deepEqual(device.commands.at(-1),["TUNING_CAPTURE",17,"fast"]);
  device.experiment="RUNNING";tuning.ctx.motorState="RUN";
  await tuning.get("refreshExperiment()");
  assert.equal(tuning.get("experimentState"),"RUNNING");
  assert.equal(tuning.get("canRunMotion()"),false);
  device.experiment="COMPLETED";device.motorState="ENABLED";
  tuning.ctx.motorState="ENABLED";
  await tuning.get("refreshExperiment()");
  assert.equal(tuning.get("experimentState"),"COMPLETED");
  assert.ok(tuning.get("experimentSnapshot"));
  // The initial Phase Search before encoder configuration intentionally fails
  // and must notify the operator. A later successful retry does not erase it.
  assert.deepEqual(errors, ["Error: Encoder not ready"]);
  assert.deepEqual(device.commands.filter(c=>c[0]==="ACTION"&&c[1]==="ACTION_IDENT_APPLY").map(c=>c[2]),["rsLs","flux","jb"]);
});

test("commissioning editors honor DISABLED, ENABLED and RUN HostSchema write_state",()=>{
  const exports={};
  vm.runInNewContext(transpile(source("parameters/writeState.ts")),{
    exports,require:()=>({}),
  });
  const can=exports.canWriteInMotorState;
  const disabled={access:"rw",writeState:"disabled"};
  const notRunning={access:"rw",writeState:"not_running"};
  const live={access:"rw",writeState:null};
  assert.equal(can(disabled,"DISABLED"),true);
  assert.equal(can(disabled,"ENABLED"),false);
  assert.equal(can(disabled,"RUN"),false);
  assert.equal(can(disabled,null),false);
  assert.equal(can(notRunning,"ENABLED"),true);
  assert.equal(can(notRunning,"RUN"),false);
  assert.equal(can(live,"RUN"),true);
  assert.equal(can({access:"ro",writeState:null},"DISABLED"),false);
  for (const page of ["motor/MotorPage.svelte","encoder/EncoderPage.svelte","limits/LimitsPage.svelte"]) {
    assert.match(source(page),/canWriteInMotorState\(metadata\[symbol\], motorState\)/);
  }
});
