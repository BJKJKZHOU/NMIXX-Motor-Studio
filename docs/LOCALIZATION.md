# NMIXX Motor Studio · 界面语言资源

## 设计约束

软件默认显示简体中文，左下角状态栏的小按钮负责切换语言，当前选择保存在 `nmixx.ui.language`。UI 文案不属于电机数据模型；改变语言不得触发 Parameter WRITE、Action 或设备重连。

```text
apps/desktop/src/
├── i18n.ts                  # 语言注册表、语言切换、显示查找
└── i18n/locales/
    ├── en.ts                # 英文文案 + 稳定 MessageKey
    └── zh-CN.ts             # 简体中文文案
```

页面、参数编辑器与通讯代码只引用消息键，不持有翻译表。静态界面文本调用 `translate("ui.motor.parameters", $uiLanguage)`；使用**当前资源中实际存在的键**。不把英文显示句子作为业务变量或协议枚举。

固件动态提供的 HostSchema `label`、运行时人类可读故障等不属于静态 UI 词条：由 `localize(label, language)` / `localizeFeedback(message, language)` 使用英文原文查找已知翻译，未知内容保持原文显示。原始日志内容和脚本源码不翻译。

**不能翻译**：`PARAM_*`、`ACTION_*`、原始枚举 wire 值、`Rs`/`Ld`/`Lq`/`Flux`/`Id`/`Iq`/`Kp`/`Ki`、工程单位（A、V、rad/s、RPM、N·m 等）。按钮 `DISABLED`/`ENABLED` 保留固件状态含义与原字母拼写。

## 新增语言（以日语为例）

1. 复制 `i18n/locales/zh-CN.ts` 到 `i18n/locales/ja.ts`，翻译右侧字符串。保持消息键不变。
2. 在新文件里使用 `satisfies Record<keyof typeof enMessages, string>`，由 TypeScript 检查词条是否缺失或存在多余键。
3. 在 `i18n.ts` 导入该词典，向 `languageOptions` 增加一行 `{code:"ja", label:"日", name:"日本語", messages:jaMessages}`。
4. 无需改动页面、按钮逻辑或任何固件协议模块。语言切换按注册表顺序循环，并自动支持新的语言值恢复。

需要增加/更改一个静态文案时，先在 `enMessages` 新增稳定键，然后在所有已有语言文件加入对应翻译，最后在页面引用 `translate("ui.xxx", $uiLanguage)`。不要从业务函数直接使用某一种语言的对象。

## 回归验证

```bash
cd apps/desktop
npm ci
npm run test:runtime
npm run build
```

`tests/i18n.test.cjs` 验证默认中文、合法的持久化选择、语种切换、两语言消息键一致、原始工程符号保持以及切换不触发电机命令。语言切换不替代真实 WebView 的文案截断、布局与字体渲染检查。
