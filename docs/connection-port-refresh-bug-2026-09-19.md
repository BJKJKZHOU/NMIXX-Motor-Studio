# USB 端口刷新与错误端口连接卡死（2026-09-19）

## 结论

这是两个相互放大的缺陷：Linux 上把易变化的 `/dev/ttyACM*` 名称当作设备身份，
刷新后可能继续保留已经失效的旧端口；随后在旧端口上执行 Connect，后端会同步读取
全部参数并忽略单项错误，导致连续等待串口超时，GUI 表现为卡死。拔掉开发板 USB 后，
阻塞的 I/O 被内核错误打断，GUI 才退出卡死状态。

本次修复基于实际复现、系统调用跟踪和正确端口对照，不是根据界面现象推测。

## 环境

- 日期：2026-09-19
- 分支：`feat/gui-converged-clean`
- 修复前基线：`678f88543bdb148cc426ba731555d587c0b02fe9`
- Schema：`/home/zhouheng/GitHub_Pro/AxDr_L_Motor/build/host/axdr-host-schema.toml`
- 开发板 USB：VID:PID `0483:5710`，序列号 `000000000001`
- 稳定设备链接：`/dev/serial/by-id/usb-STMicroelectronics_STM32_USB_Device_000000000001-if00`

## 复现步骤与现象

1. 开发板首次枚举为 `/dev/ttyACM0`，GUI 端口框显示该名称。
2. USB 重新枚举后，开发板实际变为 `/dev/ttyACM1`。
3. 点击端口刷新，GUI 仍可能保留 `/dev/ttyACM0`，只有重启软件后才显示
   `/dev/ttyACM1`。
4. 在旧的或错误的 `/dev/ttyACM0` 上点击 Connect，GUI 卡死。
5. 拔掉开发板 USB 后，上位机报告 I/O 错误并退出卡死；重新插入后，使用正确端口
   可以正常连接。

正确端口 `/dev/ttyACM1` 是对照组：点击 Connect 不会卡死，参数响应正常。

## 已抓取的证据

### USB 重编号

内核事件时间线中观察到：

- 13:00:32：USB 断开；
- 13:00:37：STM32 重新枚举为 `ttyACM0`；
- 13:02:02：再次断开并重新枚举为 `ttyACM1`。

同一块设备的 `/dev/serial/by-id/...000000000001-if00` 链接保持不变，因此
`ttyACM0`/`ttyACM1` 只是动态分配的节点名称，不能作为持久设备身份。

### 错误端口上的 Connect 阻塞

对错误的 `/dev/ttyACM0` 连接进行 `strace` 跟踪后确认：

- schema 成功加载，串口也能打开；
- 第一帧 AXDR 请求成功写入；
- 第一笔读取等待约 2.000 秒后超时；
- 后续写入反复等待 `POLLOUT`，每次约 3.003 秒；
- 一次复现中出现约 40 次连续的 3 秒发送超时；
- GUI/主线程同时等待 futex，因而窗口不再响应；
- 拔掉 USB 后，等待被 I/O 错误打断，连接流程退出。

代码检查与跟踪结果一致：原 `device_connect` 是同步 Tauri command；连接后调用
`parameter_refresh_all`，schema 中 93 个参数有 82 个可读参数；批量读取保留错误并
继续处理后续参数，而 `device_connect` 又丢弃了每一项读取结果。

### 正确端口对照

在 `/dev/ttyACM1` 上使用同一 schema 连接时，AXDR 请求得到响应，跟踪到的响应等待
约为 0.1–0.2 ms，Connect 正常完成。由此排除了“只要点击 Connect 就必然卡死”以及
“schema 本身无法加载”的解释。

## 根因

1. 端口枚举向 GUI 暴露 `/dev/ttyACM*` 动态名称。设备重新枚举后名称可能变化，
   刷新逻辑仍会保留旧选择，造成用户看到的端口与实际开发板不一致。
2. `device_connect` 在 Tauri GUI 事件线程执行同步、阻塞式串口初始化。
3. 初始化批量读取所有参数，即使单个请求已经超时仍继续后续请求，并且连接代码忽略
   每项错误，把一次通信失败放大为很长的连续超时。
4. 无响应设备可能在 tty 输出队列中留下数据，关闭串口时还会继续等待队列，延迟错误
   返回。

## 修复

- Linux 端口枚举把 `/dev/ttyACM*` 和 `/dev/ttyUSB*` 映射到匹配的
  `/dev/serial/by-id/*`，并排序、去重；系统没有对应链接时才回退到原 tty 路径。
- `device_connect` 改为异步 Tauri command，使阻塞初始化不再占用 GUI 事件线程。
- 初始参数读取改为 fail-fast：第一个读错误立即终止连接，并在错误中报告参数 symbol
  和 ID，不再继续剩余的几十次请求。
- `UsbCdcTransport` 释放时丢弃未发送的输出缓冲，避免错误路径在关闭 tty 时继续等待。
- 增加 Linux 单元测试，覆盖“优先稳定 by-id 链接”和“无匹配时回退 tty”两条路径。

本修复不修改 AXDR 协议、固件或电机控制逻辑，也不会发送电机启动 Action。

## 验证记录

| 验证项 | 结果 |
| --- | --- |
| `cargo test --workspace --all-targets` | PASS；51 项通过（含新增的 2 项端口测试） |
| `npm run build` | PASS；保留既有 a11y 和 bundle size 警告 |
| Linux by-id 映射单元测试 | PASS；2 项通过 |
| 实机 `device list` 显示稳定 by-id 路径 | PASS；仅返回 `/dev/serial/by-id/usb-STMicroelectronics_STM32_USB_Device_000000000001-if00` |
| 稳定 by-id 路径只读连接 | PASS；读取 `PARAM_MOTOR_STATE = 0`、`PARAM_ADC_VBUS = 3.553 V` |
| 旧/错误端口失败后 GUI 仍可响应 | 待 GUI 回归 |
| USB 重编号后刷新仍指向同一 by-id 路径 | 待重新插拔回归 |

## 独立观察：调试启动白屏

调试环境使用 `LC_ALL=C.UTF-8` 时，WebKit 报告的语言值为 `C`；前端 uPlot 初始化
`Intl.NumberFormat` 时会因该 locale 非法而中止渲染，表现为白屏。这与 Connect
错误端口卡死不是同一问题。使用以下命令启动可正常显示 GUI：

```bash
LC_ALL=zh_CN.UTF-8 LANG=zh_CN.UTF-8 LANGUAGE=zh_CN npm run tauri dev
```

## 后续同类审查：设备操作无响应导致 GUI 卡死

### Encoder Protocol 实机现象

在 Encoder 页把 `Encoder Protocol` 从 `SPI` 改为 `None` 后，界面仍显示旧的
`SPI` 并卡死。卡死现场的线程快照显示：GUI 主线程等待在 `futex`，
`nmixx-device-session` 线程等待在串口 `poll`。系统的 ptrace 策略禁止对运行中进程
附加 `strace`，因此没有把该线程快照扩大解释为具体的 read 或 write 系统调用。

终止 GUI、释放串口后，使用 CLI 只读 `PARAM_ENCODER_PROTOCOL` 仍得到
`device request timed out`。这证明切换后开发板仍保留 USB 串口节点，但整个 AXDR
请求链路不再响应；不能把问题解释成单纯的下拉框渲染错误。

当前固件源码中，`PARAM_ENCODER_PROTOCOL` 的写入顺序为：写值、执行
`Encoder_Config_Changed()`、随后调用 `Protocol_Parameter_Write_Response()`。但是板上
实际烧录版本未确认，且实机没有返回响应，所以不能仅凭当前源码宣称板上必然执行了
同样的回包路径。

### 上位机同类风险审查

审查所有 Tauri command 后确认，共有 25 个连接或设备 I/O 命令可能等待串口响应。
原来除 `device_connect` 外均为同步 command，因此以下操作遇到设备无响应时都可能
阻塞 GUI 事件线程：

- Parameter 单项/批量读取、写入和 Read All；
- Phase Search 与 Identification 的预检、启动和应用；
- 通用 Action、Enable、Disable、Stop 和 Save；
- Motion 预览、Run、Stop；
- Tuning 实验启动和停止；
- Scope 配置、Live、Pause 和 Stop；
- Disconnect 释放一个仍在等待设备的会话。

此外，顶层 Current/Speed/Position 状态每 500 ms 自动读取一次，原实现没有进行中
保护。把 I/O 移出 GUI 线程后，如果不同时防止重入，设备失联期间仍会不断堆积请求。

### 同类修复

- 上述直接或间接执行设备 I/O 的 Tauri command 全部改为异步 command；设备等待
  不再占用 GUI 事件线程。
- 纯本地 schema/cache 查询、Motion 配置 getter/setter、Scope 状态与快照保持同步，
  避免没有必要的改动。
- 保留原有的 500 ms 状态读取，并增加单飞保护；上一轮读取完成前不再提交下一轮请求。
- 协议仍保留明确的成功、设备错误和超时结果；异步化不把“没有响应”伪装成成功。

### 同类验证

| 验证项 | 结果 |
| --- | --- |
| `cargo test --workspace --all-targets` | PASS；51 项通过 |
| `npm run build` | PASS；保留既有 a11y 和 bundle size 警告 |
| 无响应设备上的 Encoder Protocol 操作 | PASS（GUI 响应性）；WRITE 后设备协议失联，设置本身未确认成功 |
| 其他 Parameter/Action/Motion/Scope 无响应路径 | 代码审查完成；需逐类负面回归 |

### Encoder Protocol 修复后跟踪结果

修复版 GUI 下再次选择 `None`，窗口仍可切换页面，UI 线程隔离验证通过。启动时
`strace` 同时记录到：

- 14:19:25.202，主机完整写出 13 字节请求，内容对应
  `PARAM_ENCODER_PROTOCOL (0x0301) = 0`；
- 写系统调用成功返回 13，证明请求已经进入 tty；
- 随后没有收到该 WRITE 的协议响应；
- 2 秒后开始的状态读取也没有响应；
- 后续 tty 输出队列填满，`POLLOUT` 每次等待约 3.003 秒后超时。

前端 `setU8` 在 WRITE 失败时会恢复写入前的已确认值，因此下拉框一段时间后从
`None` 返回 `SPI` 是错误回滚，不是页面默认值覆盖。没有成功响应和回读证据时，
上位机不能把这次设置显示为成功。

此次跟踪还发现，500 ms 状态刷新虽然已有单飞保护，但一轮结束后仍会继续下一轮。
状态刷新现改为逐项 fail-fast；首个通信错误立即停止自动轮询，避免设备失联后继续
填充 tty 输出队列。

### SWD 现场：Encoder DMA 中断活锁

在开发板保持上述无响应状态时，通过 CMSIS-DAP/SWD 连接，只短暂停核读取现场；
检查过程没有复位、下载或烧录固件，每次读取后均恢复运行。使用带调试符号的
`AxDr_L_Motor.elf` 解析地址，三次独立采样得到：

- CPU 三次都处于 `Handler External Interrupt(15)`，对应
  `DMA1_Channel5_IRQHandler`/`Encoder_DMA_IRQHandler`；
- 首次调用栈显示，被该中断打断的 Motor 线程正执行
  `Protocol_Parameter_Write_Response()`，即参数写处理已经进入返回响应的路径；
- Cortex-M `CFSR=0`、`HFSR=0`、`DFSR=0`、`AFSR=0`，排除 HardFault、
  BusFault、UsageFault 等异常停机；
- `Drv_IRQHandler=0`，与 `Encoder Protocol=None` 分支已经执行一致；
- `DMA1 ISR=0x00570000`，Channel 5 的 `GIF/TCIF/HTIF` 仍置位；
- `DMA1 Channel 5 CCR=0x000025ab`，其中 DMA 使能以及 `TCIE/HTIE/TEIE`
  中断使能仍然开启；`CNDTR=1`；
- NVIC `IABR0=0x00008000`，确认 IRQ15 持续处于 active；后两次采样时 DMA
  标志和配置值完全不变。

源码与寄存器现场形成闭环：`Encoder_DMA_Config()` 的 `None` 分支把
`Drv_IRQHandler` 清零后直接返回，没有停止 DMA1 Channel 5，也没有清除其中断标志；
而 `DMA1_Channel5_IRQHandler()` 提前调用 `Encoder_DMA_IRQHandler()` 后直接
`return`。函数指针为空时 ISR 不再执行原驱动的清标志逻辑，DMA 中断因此立即重入，
形成 IRQ 活锁。低优先级线程无法继续，写响应及后续 USB 协议处理均无法完成。

因此，开发板不是进入 Cortex-M Fault，而是处于 Encoder DMA 中断活锁；对上位机而言
等效为“板端卡死”。上位机的异步 command 与停止轮询修复只能保证 GUI 可响应，不能
恢复已经陷入该中断活锁的固件。固件侧还需要在切换 Encoder 协议前安全停止 SPI/DMA、
关闭 Channel 5 中断并清除 pending flags，再清空驱动函数指针。

### 固件修复与实机回归

固件提交 `23f616bf06962ef316f3ad6a8f12284ccc966ea6`
（`fix(encoder): handle None as a DMA-safe driver state`）完成板端修复：

- `None_Config()` 关闭 SPI1 RX DMA 请求和 DMA1 Channel 5；
- 关闭 Channel 5 的 `TCIE/HTIE/TEIE` 中断使能并清除全部 Channel 5 标志；
- `None_IRQHandler()` 兜底清除 Channel 5 标志，避免切换瞬间的残留中断形成活锁；
- `None` 使用完整的空驱动绑定，不再通过清空 ISR 函数指针表示该状态。

新固件下载后，使用稳定 by-id 设备路径和同一 Host schema 完成原触发路径回归：

| 验证项 | 结果 |
| --- | --- |
| 新固件基线读取 `PARAM_ENCODER_PROTOCOL` | PASS；返回 `0 (None)` |
| Motor State 写前检查 | PASS；返回 `0 (Disabled)` |
| CLI 写 `SPI` 后回读 | PASS；WRITE 返回 `OK`，回读为 `1` |
| CLI 写 `None` 后回读 | PASS；WRITE 返回 `OK`，回读为 `0` |
| 写 `None` 后继续读取 Motor State | PASS；返回 `0`，AXDR 通信保持正常 |
| GUI 连接后切换 Encoder Protocol 为 `None` | PASS；设置正常生效，界面保持响应 |

回归结果表明板端 DMA 中断活锁已修复；GUI 不再因为该操作进入协议超时与设置回滚。
