# AxDr_L 第一轮实机验证（2026-09-13）

最新结果：只读、正式 CLI、默认 FAST 采集和采集后只读已通过；用户随后提供的
control smoke 日志确认参数写入、回读、恢复及同步 Action 测试通过。
第三次测试使用 NMIXX `9075e37`，FAST 收到 2,000 帧、存储 20,000 个双通道
采样点、序号丢帧 0；当时补跑的软件测试有 3 项超时失败，详见对应记录。

## 环境与版本

- NMIXX commit：`cf4f6c452e740657b332cc2875a14c777cc33b3a`。
- 固件源码：`/home/zhouheng/GitHub_Pro/AxDr_L_Motor`。
- 固件源码分支：`feat/ccmram-fast-code`。
- 固件源码 commit：`a05b31208037c01b9e24a3f347d950bcc2215c97`。
- 板上实际烧录 commit 尚未确认；源码 HEAD 不代表已烧录版本。
- 目标串口：`/dev/ttyACM1`，STM32 USB Device，VID:PID `0483:5710`。
- `/dev/ttyACM0` 属于 MICROLINK CMSIS-DAP，未对该端口发送协议请求。
- 波特率参数：115200。
- 沙箱内 `/dev` 不暴露串口；实机命令经批准在沙箱外运行。

## Schema

现有 `build/host/axdr-host-schema.toml` 的参数内容与当前源码一致，仅
`source.git_sha` 旧于源码 HEAD。已在固件仓库运行：

```bash
python3 -B tools/export_host_schema.py
```

随后 `--check` 校验通过。未切换分支、pull 或烧录固件。

## 实测结果

各命令串行执行。正式 CLI 和 FAST 使用上述固件仓库中的 schema。

| 阶段 | 结果 |
| --- | --- |
| `nmixxctl device list` | 成功枚举，结合 sysfs USB 身份确认 ACM1 |
| `nmixx-hw-smoke --port /dev/ttyACM1` | PASS；状态 0，VBUS 14.533 V |
| `nmixxctl ... param get PARAM_MOTOR_STATE` | 成功；0 |
| `nmixxctl ... param get PARAM_ADC_VBUS` | 成功；14.601048470 V |
| `nmixx-plot-smoke ...`，默认 1 秒、Ia/Iq | FAIL，退出码 1 |
| FAST 后再次 `nmixx-hw-smoke` | PASS；状态 0，VBUS 4.991 V |

状态 0 在当前源码中为 DISABLED。未发送参数写入或电机启动 Action。
VBUS 两次阶段间明显变化，供电变化或复位情况尚待用户确认，不能据此宣称
电压测量精度已验证。

FAST 原始输出：

```text
opening /dev/ttyACM1 @ 115200
FAST Plot channels: PARAM_ADC_IA, PARAM_RUN_IQ
capture: 1000 ms @ 20000 Hz
FAIL: waiting for FAST Plot data failed: timed out waiting on channel
```

程序已越过 CONFIG/START 的成功响应检查，进入采集循环。错误路径尝试
发送 STOP，但忽略 STOP 的返回值，因此本次日志不能证明 STOP 收到成功响应。
失败路径未打印帧数与样本数，不能将此错误解读为“完全没有收到 FAST 帧”。

## 定位发现

`crates/nmixx-app/src/session.rs` 的 `Worker::run` 每轮先对命令队列执行
`recv_timeout(20 ms)`，只有等待超时才调用一次 `poll_one`。采集期间无新命令时，
即使串口已有积压帧，也最多约处理 50 帧/秒。默认 FAST 需要存储 20,000 个
双通道采样点，在约 3 秒期限内无法靠这一接收速率完成。这是已确认的代码吞吐
瓶颈，但本轮尚未通过修复前后对照排除其他实机问题。

固件当前源码在 `Fast_Loop` 的 `FAST_OFF` 分支也调用 `Plot_Fast_Sample()`，
因此不能仅凭 DISABLED 状态认定固件不会产生 FAST 数据。

尝试现有 Python 工具作 1 秒同通道对照：

```bash
python3 -B /home/zhouheng/GitHub_Pro/AxDr_L_Motor/tools/plot_test.py \
  --port /dev/ttyACM1 --fast Ia Iq --seconds 1 --hex-data 1
```

工具在打开串口之前因以下错误退出，未得到对照采集数据：

```text
ImportError: cannot import name 'PARAM_RUN_THETA_M' from 'parameter_ids_generated'
```

## 后续验证

1. 修复会话线程每帧前等待命令队列的瓶颈，并用连续异步帧回归验证吞吐及命令响应。
2. 补充 FAST 失败时的统计输出及 STOP 结果报告，覆盖解码失败等退出路径。
3. 重跑默认 FAST；检查 20,000 个存储采样点、零序号丢帧及 STOP 成功响应。
4. 再做只读复测，并确认供电条件及实际烧录版本。

本轮未修改 Rust 或固件源码。`MAX_CANFD_DATA_LEN` 未使用警告保留。

## 同日第二次测试

按用户要求，在未修改程序源码的情况下重新串行执行完整流程。
目标 USB 身份仍为 STM32 `0483:5710`、`/dev/ttyACM1`；schema 的 `--check`
再次通过。

| 阶段 | 第二次结果 |
| --- | --- |
| 串口枚举 | 成功 |
| 最小只读 smoke | PASS；状态 0，VBUS 5.008 V |
| 正式 CLI 读取状态 | 0 |
| 正式 CLI 读取母线电压 | 5.008007526 V |
| 默认 FAST，1 秒、Ia/Iq | FAIL；`waiting for FAST Plot data failed: timed out waiting on channel` |
| FAST 后只读 smoke | PASS；状态 0，VBUS 4.991 V |

FAST 超时可重复出现，只读通信保持正常。本次离散读数均约 5 V。
会话线程的每帧 20 ms 命令等待逻辑仍未修改；尚不能判定 FAST 链路通过。
与首次测试相同，失败路径未输出采集统计，STOP 的返回结果也未记录。

## 同日第三次测试：接收循环修复后

- NMIXX commit：`9075e3732c7768d6584f4fa95181dc5c6c494628`。
- 已确认 `Worker::run` 使用 `commands.try_recv()`，移除了逐帧命令等待。
- 固件源码 HEAD 仍为 `a05b312`；schema `--check` 通过。
- USB 身份及目标串口保持不变；重新编译 CLI 和两个 smoke 程序后串行测试。

| 阶段 | 第三次结果 |
| --- | --- |
| 串口枚举 | 成功，目标 ACM1 |
| 最小只读 smoke | PASS；状态 0，VBUS 4.974 V |
| 正式 CLI 读取状态 | 0 |
| 正式 CLI 读取母线电压 | 4.991088390 V |
| 默认 FAST，1 秒、Ia/Iq | PASS；2,000 帧、20,000 个采样点、零序号丢帧 |
| FAST 后只读 smoke | PASS；状态 0，VBUS 4.974 V |

FAST 原始输出：

```text
opening /dev/ttyACM1 @ 115200
FAST Plot channels: PARAM_ADC_IA, PARAM_RUN_IQ
capture: 1000 ms @ 20000 Hz
frames received: 2000
samples received: 20000
samples stored: 20000
sequence lost frames: 0
PARAM_ADC_IA (0x0001): min=-0.081000 max=0.121000
PARAM_RUN_IQ (0x0011): min=0.241000 max=0.241000
PASS: Plot CONFIG/START/FAST DATA/STOP and RAM capture are working
```

此次正常完成路径会检查 STOP 响应，故 PASS 覆盖 CONFIG、START、FAST 数据、
STOP 和 RAM 采集。20 kHz 为主机配置值，本轮未单独测量实际采样频率。
Iq 在采集内恒为 0.241 A，只记录观测值，不据此宣称电流测量或闭环控制已验证。

同时补跑两个软件测试目标：

| 命令 | 结果 |
| --- | --- |
| `cargo test -p nmixx-app --test session_runtime` | 4 通过、2 失败 |
| `cargo test -p nmixx-app --test plot_runtime` | 1 通过、1 失败 |

失败项均为 `Timeout`：

- `timeout_cancels_pending_transaction_and_next_request_can_succeed`
- `malformed_response_does_not_poison_following_request`
- `plot_config_start_stream_and_stop_share_single_session_owner`

新增 `fast_burst_is_dispatched_without_per_frame_poll_delay` 本次通过。
失败用例的模拟传输在请求发送前预置响应；修复后的接收线程可能在对应请求提交前
消费响应，存在测试时序竞态。这是代码检查得到的优先排查方向，尚未通过修改
模拟传输验证根因，不能宣称软件回归全通过。

本次仅更新测试记录，未修改 Rust/固件源码、未烧录、未发送电机启动 Action。
第一轮实机通信和默认 FAST 验收已通过；后续仍需处理上述软件测试失败。

## Control smoke：参数写入与同步 Action

结果来源：用户实际执行后提供的终端日志。本节据该日志整理，未重新操作硬件；
本次运行的主机 commit 与板上固件 commit 未由日志给出。

执行命令：

```bash
cargo run -p nmixx-cli --bin nmixx-control-smoke -- \
  --port /dev/ttyACM1
```

程序输出（按行整理）：

```text
opening /dev/ttyACM1 @ 115200
PARAM_MOTOR_STATE (0x0710) = 0
Parameter WRITE: PARAM_MOTOR_PP 0x0101: 7 -> 8 -> 7
PASS: Parameter WRITE/readback/restore
Synchronous Action: ACTION_PROTECTION_CLEAR (0x1201)
accepted/completed synchronously: txn=7 action=0x1201
PASS: synchronous Action response
PASS: safe Parameter WRITE and synchronous Action handling are working
```

| 验证项 | 结果 |
| --- | --- |
| 初始电机状态 | `PARAM_MOTOR_STATE = 0`，DISABLED |
| 参数写入与回读 | `PARAM_MOTOR_PP`（`0x0101`，u8）从 7 写为 8，回读一致 |
| 原值恢复与回读 | 从 8 恢复为 7，回读一致 |
| 同步 Action | `ACTION_PROTECTION_CLEAR`（`0x1201`），事务号 7，响应通过 |
| Action 后状态检查 | 按当前 smoke 实现，输出最终 PASS 前已确认仍为 DISABLED |

判定：**PASS**。本次覆盖 Parameter WRITE、READ 回读及恢复，以及保护清除
Action 的同步响应处理。未覆盖异步 Action 完成事件、电机启动或闭环运行；
日志也未提供保护位清除前后的数值。

构建时仍出现 `MAX_CANFD_DATA_LEN is never used` 警告；编译完成且程序输出
最终 PASS，该警告未阻止本次测试。
