// UI language is presentation state, independent of firmware Parameters / Actions.
const { test } = require("node:test");
const assert = require("node:assert/strict");
const { readFileSync } = require("node:fs");
const { join } = require("node:path");
const vm = require("node:vm");
const ts = require("typescript");

const root = join(__dirname,"../src");
const source = path => readFileSync(join(root,path),"utf8");
function loadLocale(path) {
  const exports = {};
  vm.runInNewContext(ts.transpileModule(source(path), {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS }
  }).outputText, { exports, require: () => { throw Error("Locale files must contain text only"); } });
  return exports;
}
const english = loadLocale("i18n/locales/en.ts");
const chinese = loadLocale("i18n/locales/zh-CN.ts");
function environment(stored = null, storageAvailable = true) {
  let value = stored;
  const updates = [];
  const storage = {
    getItem(key) { assert.equal(key,"nmixx.ui.language"); if(!storageAvailable)throw Error("blocked"); return value; },
    setItem(key,next) { assert.equal(key,"nmixx.ui.language"); if(!storageAvailable)throw Error("blocked");value=next; },
  };
  const exports = {};
  const context = vm.createContext({
    exports,localStorage:storage,
    require(name) {
      if (name === "./i18n/locales/en") return english;
      if (name === "./i18n/locales/zh-CN") return chinese;
      assert.equal(name,"svelte/store");
      return { writable(initial) {
        let current=initial;
        return {
          subscribe(callback) {callback(current);updates.push(callback);return()=>{};},
          set(next) {current=next;for(const callback of updates)callback(current);}
        };
      }};
    },
  });
  vm.runInContext(ts.transpileModule(source("i18n.ts"),{
    compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS},
  }).outputText,context);
  let language;
  exports.uiLanguage.subscribe(next=>language=next);
  return {exports,language:()=>language,stored:()=>value};
}
test("Chinese is the first-install default and English is opt-in",()=>{
  const env=environment();
  assert.equal(env.language(),"zh-CN");
  assert.equal(env.exports.localize("Connection",env.language()),"连接");
  assert.equal(env.exports.localize("Rs",env.language()),"Rs");
  assert.equal(env.exports.localize("Flux",env.language()),"Flux");
  assert.equal(env.exports.localize("Id",env.language()),"Id");
  assert.equal(env.exports.localize("N·m",env.language()),"N·m");
  assert.equal(env.exports.localize("PARAM_MOTOR_RS",env.language()),"PARAM_MOTOR_RS");
  env.exports.setUiLanguage("en");
  assert.equal(env.language(),"en");
  assert.equal(env.stored(),"en");
  assert.equal(env.exports.localize("Connection",env.language()),"Connection");
  env.exports.setUiLanguage("zh-CN");
  assert.equal(env.language(),"zh-CN");
  assert.equal(env.stored(),"zh-CN");
});
test("restores only supported language codes and works without localStorage",()=>{
  const en=environment("en");en.exports.restoreUiLanguage();
  assert.equal(en.language(),"en");
  const unsupported=environment("jp");unsupported.exports.restoreUiLanguage();
  assert.equal(unsupported.language(),"zh-CN");
  const blocked=environment("en",false);blocked.exports.restoreUiLanguage();
  assert.equal(blocked.language(),"zh-CN");
  blocked.exports.setUiLanguage("en");
  assert.equal(blocked.language(),"en");
});
test("translation of device acknowledgements preserves English command data",()=>{
  const {localizeFeedback}=environment().exports;
  assert.equal(localizeFeedback("Run failed: Error: ERR_STATE","zh-CN"),"运行失败：Error: ERR_STATE");
  assert.equal(localizeFeedback("Run failed: Error: ERR_STATE","en"),"Run failed: Error: ERR_STATE");
  assert.equal(localizeFeedback("Motor Run accepted by device; awaiting motor-state feedback.","zh-CN"),"设备已接受 Run 命令，等待电机状态反馈。");
  assert.equal(localizeFeedback("Rs/Ls injection peak must be below the configured current limit.","zh-CN"),"Rs/Ls 注入峰值必须低于配置电流限值。");
});
test("multi-line commissioning preflight reasons localize individually",()=>{
  const {localizeFeedback}=environment().exports;
  const english="Current limit must be configured before identification.\nAlign current must not exceed the configured current limit.";
  const result=localizeFeedback(english,"zh-CN");
  assert.match(result,/辨识前需配置电流限值/);
  assert.match(result,/Align 电流不得超过配置电流限值/);
  assert.equal(result.split("\n").length,2);
});

test("bottom-left language toggle is separate from Connection and cannot send motor commands",()=>{
  const app=source("App.svelte");
  assert.match(app,/<footer class="statusbar"><button class="language-toggle"/);
  assert.match(app,/onclick=\{\(\) => cycleUiLanguage\(\$uiLanguage\)\}/);
  assert.match(app,/restoreUiLanguage\(\)/);
  assert.match(app,/\{languageBadge\(\$uiLanguage\)\}/);
  const fragment=app.match(/<button class="language-toggle"[\s\S]*?<\/button>/)?.[0];
  assert.ok(fragment);
  assert.doesNotMatch(fragment,/motor_enable|motor_disable|motor_stop|parameter_write|invoke\(/);
});
test("primary workflow pages subscribe to the shared language state",()=>{
  for(const path of [
    "connection/ConnectionPage.svelte","motor/MotorPage.svelte",
    "encoder/EncoderPage.svelte","limits/LimitsPage.svelte",
    "control/ControlPage.svelte","control/ControlTuningPage.svelte",
    "motion/MotionPage.svelte","analysis/scope/ScopeEchartsPage.svelte",
    "parameters/ParameterTablePage.svelte","events/EventsPage.svelte",
    "automation/AutomationPage.svelte",
  ]) {
    const content=source(path);
    assert.match(content,/import \{ uiLanguage, localize/);
    assert.match(content,/\$uiLanguage/);
  }
});

test("locale resources share the same stable keys and preserve engineering symbols",()=>{
  const en = english.enMessages, zh = chinese.zhCNMessages;
  assert.deepEqual(Object.keys(en).sort(),Object.keys(zh).sort());
  assert.ok(Object.keys(en).length >= 300);
  const {translate}=environment().exports;
  assert.equal(translate("ui.connection","zh-CN"),"连接");
  assert.equal(translate("ui.connection","en"),"Connection");
  assert.equal(translate("ui.imax","zh-CN"),"Imax");
  assert.equal(translate("ui.imax","en"),"Imax");
});

test("language registry adds future locales without changing statusbar button",()=>{
  const {nextUiLanguage,languageBadge,languageSwitchTitle}=environment().exports;
  assert.equal(nextUiLanguage("zh-CN"),"en");
  assert.equal(nextUiLanguage("en"),"zh-CN");
  assert.equal(languageBadge("zh-CN"),"中");
  assert.equal(languageBadge("en"),"EN");
  assert.equal(languageSwitchTitle("zh-CN"),"English");
});
