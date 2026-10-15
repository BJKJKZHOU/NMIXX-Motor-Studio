# 陌生电机首次接入：用户操作闭环模拟

## 模拟边界

这是一条覆盖 Connection → Limits → Motor → Encoder → Motion → Control Tuning 的**脚本化用户旅程**，不是把未经验证的真实电机接入自动化。测试执行仓库各页面的实际 Svelte 操作处理函数；虚拟设备返回预检阻断、Action 接受、有限 Action 完成、Parameter 回读、状态迁移等预先定义的响应。关键电流预检另用 Rust 原始 DeviceSession/ParameterService + 模拟 FrameTransport 验证。

所用数据仅用于测试分支条件，**不是推荐给陌生电机的实机电流/速度值**。极对数、允许电流、供电、电气接线与编码器协议必须由使用者确认；固件持久化配置可能属于上一台电机。所有实际 PWM、功率级、位置环和停止行为仍需台架验证。

## 模拟流程与检查结果

| 顺序 | 用户实际操作 | 上位机 / 固件语义 | UI 与可观察反馈 | 当前验证 |
|---|---|---|---|---|
| 1 | 在 Connection 选择 USB CDC 端口与 HostSchema，点击 Connect | `device_connect`；完成设备初始化与 Parameter 读回后开放业务页 | 连接端口可用，初始 Motor State 为 DISABLED，Enable 状态红色 | 页面操作函数模拟；真实端口待测 |
| 2 | 检查电机极对数、旧 NVS 电机参数；在 Limits 确认电流/转速限值 | 用户点击参数写入后，设备回读共享 RAM 值；只在 DISABLED 时允许写入需要停机的项 | 相关编辑框应可用；越权写入锁定 | 共用编辑器与 write_state 检查 |
| 3 | Motor 首次点击 Rs/Ls（脚本中注入峰值与 Imax 相等） | Application 预检必须阻断，**不得**下发辨识 Action | Failed，并显示“注入峰值必须小于限值”的具体原因 | Rust 真正预检 + 页面模拟 |
| 4 | 将 `PARAM_MOTOR_RL_I_PEAK` 配置至合适范围后再次点击 Rs/Ls | 预检 → 弹窗请求使能许可 → 写 `PARAM_MOTOR_MODE=IDENT` → Enable → `ACTION_IDENT_RS_LS_START` | Running → 有效完成后 Ready → 点击 Apply 进入 Applied；活动 Rs/Ld/Lq 随回读更新 | 页面处理器模拟；实际电机执行待测 |
| 5 | 点击 Flux，初始 Align / I/F 未配置 | 预检阻断，不能启动 Flux | Failed，直接显示缺失配置原因 | Rust 预检 + 页面模拟 |
| 6 | 当前仍 ENABLED；尝试调整 Align/I-F | 页面按照 HostSchema `write_state: disabled` 禁止发送写入，要求用户点击全局 Disable | 使能状态变红色 DISABLED 后方可修改；不得仅凭 Action ACK 乐观切换 UI | 页面联动模拟 |
| 7 | 在 Motor 设置 Align、I/F 后依次 Flux Start / Apply、J/B Start / Apply | 使用当前参数进入 IDENT；每次通过 Action 完成事件与 Valid 结果更新候选，再由 Apply 写入活动模型 | 分别 Running → Ready → Applied，Rs/Ld/Lq/Flux/J/B 使用同一数据所有权 | 页面模拟与 Rust Current 预检 |
| 8 | Encoder 首次点击 Phase Search，但反馈未 Ready | Application 预检拒绝，UI 不假装寻相成功 | Failed，直接展示 Encoder not ready | 页面处理器模拟；实际通信待测 |
| 9 | 全局 Disable → Encoder 选择正确反馈协议 / SPI 类型 → 等待反馈有效 → 再次 Phase Search | 依 Application 编排切换 PHASE_SEARCH、Enable、Run；有限 Action 返回后刷新校准结果 | Running → Success；校准有效与方向偏置须以固件回读为准 | 页面模拟；校准数值未实测 |
| 10 | Disable 后执行全局 Save | `ACTION_CONFIG_SAVE`；设备 ENABLED 时 Save 不可提交 | 保存完成后持久化基线更新；不把 Action 接受误认为通用运控完成 | 页面处理器模拟 |
| 11 | 在 DISABLED 状态从 Motion 选择 Position，填写有意义的 Max Speed、Acc/Dec，再全局 Enable | 模式 Parameter Write → 确认编码器已 Ready 和相位校准有效 → Enable | 模式页面与全局 ENABLED 状态同步；未提交草稿不得 Run | 模式切换与全局控制模拟 |
| 12 | 打开 Control Tuning，手工选择可用 Iq FAST 通道，点击 Run | `tuning_experiment_start`，准备采集后 Application 发运动命令；按有限 Position 轨迹完成与受控 Stop 规则收尾 | PREPARING → RUNNING → COMPLETED；按钮与实验状态联动，完成后保留快照 | 页面处理器模拟；实际采集路径暂缓 |

## 已发现并修正

1. Rs/Ls 电流 `PARAM_MOTOR_RL_I_PEAK` 原本不在 Motor 页面，且预检只确认了 Imax 为正，造成“界面允许启动但固件拒绝”。现在提供共享输入并检查峰值必须低于用户 Imax；硬件限值和内部注入下限仍由固件最终验证。
2. Flux/J/B 的 `PARAM_MOTOR_ALIGN_CURRENT` 原本缺失于 Motor 界面和预检；J/B 的 I/F 预检同样缺失。现已补齐用户可配置参数和必要条件。
3. Motor、Encoder、Limits 原本在 ENABLED/RUN 仍可能允许编辑固件仅接受 DISABLED 状态写入的参数。现按共享 Motor State 和 HostSchema 写权限锁定输入，避免用户误操作。
4. 辨识 / 寻相失败原因原本只藏在 `title` 悬停中；现在在页面直接显示详细错误。
5. 首次完整脚本跑通后，补充全局 Save 必须处于 DISABLED 的顺序验证。

## 尚不能判定为已验证

- **固件 Plot 能力与 Control Tuning 默认选道问题暂缓**。模拟使用用户手选、真实 HostSchema 报告可用的 Iq FAST 通道，不能据此认为默认选道已修复，也不能证明完整波形已经实际获取。
- 虚拟设备按脚本返回辨识成功，未计算真实 Rs/Ld/Lq、磁链、J/B，也未验证磁链估计器切换稳定性、寻相零位误差、位置实际到位。
- 部分安全条件仍属于固件自身：硬件电流限制、母线电压可用范围、辨识内部固定最大/最小激励等。Host 不能仅依据模拟读回声明真实安全。
- 当前上位机没有“自动识别这是一台新电机”的机制。连接旧 NVS 时，上位机不能自动判断现有电机参数属于上一台电机，用户必须人工核对，尤其首次开始可能旋转的动作前。
- 软件尚未在真实 Tauri WebView 点击遍历所有字段，且不存在有实际电机的自动 CI。模拟与硬件验收是两种不同证据。

## 回归入口

```bash
cd apps/desktop
npm ci
npm run test:runtime
npm run build
cd ../..
cargo test --workspace --locked
```

- `apps/desktop/tests/new-motor-journey.test.cjs`：一次完整跨页旅程，检查命令顺序、拦截行为和状态联动。
- `crates/nmixx-app/src/preflight.rs` 中 `onboarding_tests`：通过真实 Rust ParameterService/DeviceSession 读取模拟协议响应，验证电流配置错误及恢复。
- `docs/OPERATOR_FLOW_VERIFICATION.md`：其他页面和常规用户命令闭环检查。
