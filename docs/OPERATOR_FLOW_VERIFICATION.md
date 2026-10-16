# 用户操作闭环验证：NMIXX Motor Studio

本表验证 **用户动作 → UI 状态 → Tauri/Application → AxDr 参数/Action → 回应 → UI 反馈**。
它不是“页面存在即完成”的清单。未做过硬件测试的项目不能标记为实测通过。

## 验证等级

- **HANDLER-SIM**：在受限 JS 沙盒中提取并执行真实 Svelte 操作函数，以模拟 Tauri 返回值；没有运行整个 Svelte/WebView。
- **NODE-TEST-ADDED**：仓库已有可重放的 Node 脚本测试；当前开发分支已通过 GitHub Actions 前端全套回归，但不代表 WebView 实机验证。
- **WIRE-TEST-ADDED**：Rust 模拟 `FrameTransport` 的通信帧测试已编写，开发分支已通过 CI 的 `cargo test --workspace --locked`；未使用真实硬件。
- **CODE-TRACED**：核对 Rust Application/DeviceSession 和固件现有实现；不等于实际硬件观察。
- **HW-PENDING**：需固件开发板/电机实测；尤其是故障、运动时序与 Plot 数据路径。

## 2026-10-16 修复与测试结果

- 审查修改直接提交到唯一开发分支 `dev/nmixx-unified`，`main` 不变。
- [Operator Regression CI](https://github.com/BJKJKZHOU/NMIXX-Motor-Studio/actions/runs/38842341919)：Frontend user flows 与 Rust Application and simulated protocol 均通过。
- Motor Identification、Encoder Phase Search/Homing 只接受与正在进行操作的 `(txn, actionId)` 一致的有限 Action 完成事件；对于先到的事件，在启动响应返回 Handle 后再核对。
- 新增同类型异事务完成事件、早到完成事件、取消后迟到启动响应等回归；陌生电机模拟同步适配。
- 仍未完成：真实 Tauri WebView 点击和完整固件硬件验收；固件 Plot 与 Tuning 默认选道断点维持暂缓。

## 2026-10-16 后续审查：Action ACK 与读回边界

- 代码修复直接提交于 `dev/nmixx-unified`，不创建额外开发分支，不影响 `main`。
- [前端与 Rust 完整回归](https://github.com/BJKJKZHOU/NMIXX-Motor-Studio/actions/runs/38846695908) 均通过；该结果不代表 Tauri WebView 或电机硬件验收。
- 固件已接受的辨识 Start、Apply、寻相、Enable/Disable、Stop 等 Action 不因后续缓存同步失败而丢失 Handle。
- `synchronizeParameters()` 现在检查各可读 Parameter 的缓存错误和缺失值；完成事件携带独立 `readbackError`，Motor / Encoder 显示“未确认”，不得因此误报固件拒绝或校准成功。
- Connection 拦截重复 Connect，异步端口发现不得覆盖用户在请求期间手工输入的端口；相关用户流程模拟已加入回归。
- 目前“未确认”结果不会单凭旧 VALID 参数自动恢复为可 Apply；用户须先检查最新设备参数并重新操作。后续可单独设计明确的校验/恢复入口，不应恢复使用旧数据猜测。

## 正常通信用户操作复审（2026-10-16）

本轮优先检查无通信异常时用户动作是否按预期真正落到固件、跨页状态是否一致，以及先后 Run/Stop 的正常异步返回是否产生错误反馈。

- Motor 辨识 Start/Apply、Encoder Phase Search/Homing 在存在未提交草稿或共享 Parameter WRITE 尚未完成时不再下发 Action；UI 提示 Enter 写入或 Esc 放弃。避免画面显示新值而固件使用上一有效 RAM 值。
- Parameters 专家页编辑权限与 Motor/Encoder/Limits 使用同一个 HostSchema `write_state` 判断，并订阅全局设备 Motor State；在 DISABLED/ENABLED/RUN 间变化时禁用不允许的写入。
- Motor、Encoder 在导航切换时保持挂载，有限 Action 完成事件订阅及 Running/Ready/Success 等状态不因离开页面丢失；设备连接代次变化时仍按已有逻辑重置。
- Control Tuning 勾选 Copy Accel to Decel 不再产生无缘由的 Decel 草稿；实际编辑 Accel 后继续成对提交。
- Motion、Control Tuning 以用户的最新 Run/Stop 操作为 UI 反馈依据：Stop 已发起时，较早的 Run IPC 迟到响应不得把停止提示覆盖为“Run 已接受”。
- 覆盖新增：实际 Svelte Handler 模拟、跨页 Journey、专家页 Write-State 判断、挂载边界，以及 Run/Stop 交错的前端回归。持续以 GitHub Actions 前端 `test:all`/`build` 与 Rust Workspace 回归为证据。

**尚未验证**：完整 Tauri WebView 实际点击、真实固件、电机动作和完整 Plot/Tuning 采集。固件 Plot 能力与默认采集通道断点仍按约定暂缓。正常控制闭环审查也不意味着所有页面已通过实机验收。

## 用户操作与反馈矩阵

| 用户操作 | 预期设备命令 / 实际代码路径 | 成功反馈与 UI 联动 | 失败及取消 | 覆盖等级 |
|---|---|---|---|---|
| Motion 修改 Delta 但未按 Enter，点击 Run | **不得发送任何命令**；必须先提交或放弃草稿 | Run 不可用，显示 Enter/Esc 的提交说明 | 即使程序调用处理器，也必须阻断并给出错误 | HANDLER-SIM、NODE-TEST-ADDED |
| Motion 第一次 Position Run（Incremental） | 后端从精确 `PARAM_RUN_POSITION` 生成目标 → `PARAM_TARGET_POSITION` WRITE → `ACTION_MOTOR_RUN` | Tauri 返回 `runAccepted`，显示命令已接受；后续 `Motor State` 仍由设备状态决定 | 拒绝 Action 时显示 `Run failed`，不得显示完成 | HANDLER-SIM、WIRE-TEST-ADDED、HW-PENDING |
| Motion Repeat 第二次 Run（电机保持 RUN） | 上位机先确认上一段参考到达目标，仅 WRITE 反向 `PARAM_TARGET_POSITION`；不发送第二个 Run Action | Tauri 返回 `targetUpdated`，显示已更新目标，**不宣称运动已完成** | 尚未到达目标或 WRITE 失败时不得推进 A/B | HANDLER-SIM、WIRE-TEST-ADDED、HW-PENDING |
| Motion Stop / 全局 Stop | `ACTION_MOTOR_STOP`；固件按原减速逻辑回到 ENABLED | 显示 Stop 已接受 + 最新 Motor State；`RUN` 仍可能持续一段时间 | ERR_STATE 或读回失败须分别报告；Stop ACK 不等于已停稳 | HANDLER-SIM、NODE-TEST-ADDED、CODE-TRACED、HW-PENDING |
| 全局 Enable / Disable | `DISABLED` 时点击发送 `ACTION_MOTOR_ENABLE`，`ENABLED` 或 `RUN` 时点击发送 `ACTION_MOTOR_DISABLE` | 按钮常态只显示固件当前状态：红色 `DISABLED` 或绿色 `ENABLED`（RUN 仍为使能）；Tooltip 标识点击动作，ACK 后等真实 Motor State 更新 | 设备拒绝或读回失败时不预先切换按钮状态；状态未知时显示中性 UNKNOWN | NODE-TEST-ADDED、HW-PENDING |
| Motor Rs/Ls 等辨识 Start | 应用预检；需使能时先征求确认；被允许后设置 IDENT 模式、Enable、发相应 `ACTION_IDENT_*_START` | 页面进入 Running；收到**匹配的有限 Action**完成事件与有效结果后显示 Apply | 预检 Blocked 显示原因；取消使能不发命令；Action 失败显示 Failed | HANDLER-SIM、NODE-TEST-ADDED、CODE-TRACED、HW-PENDING |
| Motor Apply | `ACTION_IDENT_APPLY` 同步完成，依当前 Session 的候选辨识类型执行 | 后端刷新真实参数，再显示 Applied | 若候选失效或被拒绝，不显示 Applied | HANDLER-SIM、NODE-TEST-ADDED、HW-PENDING |
| Encoder Phase Search | 应用预检与模式/使能编排 → `ACTION_MOTOR_RUN`（PHASE_SEARCH） | GUI 显示 Running；有限 Action 完成后为 Success | 启动拒绝或完成失败显示 Failed；全局 Stop 为 Stopped | HANDLER-SIM、NODE-TEST-ADDED、CODE-TRACED、HW-PENDING |
| Control Tuning Run | `tuning_experiment_start` → 采集准备与预采集 → Application Motion Run；Position 由实际参考到位 + 稳定窗口触发 Stop | UI 轮询 PREPARING → RUNNING → STOPPING → COMPLETED/FAILED，保留已有波形 | 未提交草稿不启动；故障与用户 Stop 取消 A/B | HANDLER-SIM、NODE-TEST-ADDED、CODE-TRACED、HW-PENDING |
| Control Architecture/Tuning 修改 Source 或增益 | 通过共享 Parameter WRITE，固件更新 Bandwidth/Manual 相关状态，依 HostSchema 回读依赖值 | 编辑控件回到固件规范值，Speed Effective 独立显示，不将 Manual 误称实际增益 | 固件拒绝保持真实错误状态；依赖新 HostSchema 分支 | CODE-TRACED、HW-PENDING |
| Parameters / Limits 按 Enter | 单个 Parameter WRITE、Application 规范读回、共享缓存更新所有订阅页面 | 当前 RAM 值与跨页控件一起变化；持久化修改显示直到 Save | Esc/失焦不应发送写操作；写入拒绝显示错误，不虚报成功 | CODE-TRACED、既有单元测试、HW-PENDING |
| Connection | `device_connect` 初始化 Session、读取参数与发现能力 | 成功初始化后才开放业务页；失败保留 Stop/Disconnect 并显示重试 | 不显示假 Connected、不对故障端口自动 Enable | CODE-TRACED、既有连接模拟、HW-PENDING |
| Events：Recheck / Clear Fault / Quick Action | Recheck 读取当前 Protection/Encoder 状态；Clear 仅调用 Protection Clear；Quick Action 只负责导航 | Active/History 由后端状态更新，导航至 Encoder/Limits | 导航不消除故障、Clear History 不清除仍在 Active 的问题 | CODE-TRACED、既有单元测试、HW-PENDING |

**排除范围：** 当前明确暂缓固件 Plot 通道与 Control Tuning 默认采集集合的兼容问题。本矩阵不因模拟采集状态而将真实 Scope/Tuning 数据路径记为通过。

## 可执行回归

在仓库根目录执行：

```bash
cargo test --workspace
cd apps/desktop
npm ci
npm run test:runtime
npm run test:parameters
npm run test:automation
npm run test:plots
npm run check
```

- `apps/desktop/tests/motion-operations.test.cjs`：真实 Motion/Tuning 页面处理函数 + 假 Tauri IPC；含拒绝、草稿和状态变化。
- `apps/desktop/tests/commissioning-operations.test.cjs`：真实 Motor/Encoder 处理函数的有限 Action/Apply 反馈。
- `apps/desktop/tests/global-motor-actions.test.cjs`：全局 Enable/Disable/Stop 接受与失败提示。
- `crates/nmixx-app/tests/session_runtime.rs`：DeviceSession 与模拟 FrameTransport 的 Position WRITE / Action 帧内容及顺序。
- `crates/nmixx-app/src/parameter_service.rs`：规范回读及失败恢复测试。

上述模拟不能覆盖：Svelte 编译器的事件绑定、实际 Tauri IPC 线程时序、固件控制周期、编码器与功率级行为，以及保护故障实机复现。这些需要另做 WebView 和硬件验证。

## 操作反馈基本规则

设备 Action 的同步接受响应，只证明设备已接受命令；绝不当作普通 Position、Speed 的运动完成事件。Stop 受控减速结束以 Motor State 为准。辨识和 Phase Search 的有限 Action 完成才可以根据对应完成事件标记 Ready/Success。参数写入成功后的读回失败必须显示“命令已接受但读回失败”，不能把设备 ACK 错报为拒绝。
