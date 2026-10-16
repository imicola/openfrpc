# Review 修复对照表（R01–R31）

对应 2026-10-16 的一次内部深度 Code Review（31 项：4 项 P1、27 项 P2）。
本文件记录每项的处置方式与验证证据。回归测试：`cargo test`（30 项）+ `node scripts/test-format.mjs`（7 组断言）。

## P1

| # | 问题 | 修复 | 验证 |
| --- | --- | --- | --- |
| R01 | 内联 TOML 表触发 `as_table_mut().unwrap()` panic | config.rs 新增 `table_like_mut()`，auth/transport/tls/代理 transport 四处全部经 TableLike 视图写入，内联表与普通表同路径 | `inline_auth_and_tls_tables_do_not_panic`、`inline_proxy_transport_table_writable` |
| R02 | 端口区间解析可无限循环卡死 UI | format.js `parsePortSpec` 解析后立即 `Number.isSafeInteger` 校验并裁剪到 1..65535 再进循环；超长数字（>15 位）直接跳过 | `node scripts/test-format.mjs`（含 9007199254740992-…94 用例 <500ms 返回） |
| R03 | 并发启动产生失管 frpc 进程 | frpc.rs 新增 `lifecycle` 互斥锁串行化 start/stop/restart/自动重启的 spawn 决策；子进程 `kill_on_drop(true)` 兜底 | 静态时序收敛：所有 spawn 路径（手动/托盘/自动启动/监督器重试）持锁后重检 `managed` |
| R12 | 「新建代理」静默覆盖同名条目 | `upsert_proxy` 拆分语义：`old_name=None` 为 Create，重名必须失败；Update 目标消失报错不退化创建；重命名查重。前端提交前也预检 | `create_existing_name_fails_without_touching_entry`、`rename_to_existing_fails`、`update_missing_target_fails_not_falls_back_to_create` |

## P2 — A 组：配置完整性与输入边界

| # | 问题 | 修复 | 验证 |
| --- | --- | --- | --- |
| R04 | TLS 布尔误读为整数、关闭开关被删除回落缺省 | `tls_enabled: Option<bool>` 三态建模；按 `as_bool()` 读取；显式写 true/false；缺省显示按 frp 0.50+ 有效默认（启用） | `tls_true_false_absent_roundtrip` |
| R06 | 同秒备份互相覆盖 | 备份名毫秒精度（`%3f`）+ 已存在时追加 `-N` 序号 | `rapid_backups_keep_both_versions` |
| R13 | HTTP 认证写成不存在的 `basicAuth` 数组 | 改用上游 `httpUser`/`httpPassword`（仅 HTTP 类型展示；HTTPS 无此字段）；旧 basicAuth 解析迁移并在写盘时替换 | `http_auth_written_as_upstream_keys`、`legacy_basicauth_parsed_and_migrated` |
| R14 | 切换代理类型残留旧类型字段 | 前端按类型显式清空 subdomain 等字段（原遗漏 subdomain）；后端 `apply_entry` 末尾按类型清理非法键兜底 | `switching_http_to_tcp_cleans_type_specific_fields` |
| R19 | 内联数组 proxies 静默为空；plugin 条目被强填 localPort=0 | 内联数组支持读取并警告「仅查看」；plugin 条目保留原字段，表单隐藏本地地址/端口且跳过校验，写回不触碰 | `inline_proxies_array_parsed_with_warning`、`plugin_entry_keeps_local_port_untouched` |
| R21 | 保存无版本检查、非原子写 | `text_rev`（FNV-1a）内容版本号随 ConfigInfo/SaveResult 下发；`config_save_text` 接受 `expectedRev`，不一致返回冲突错误；配置与 settings.json 均走 `atomic_write`（临时文件 + rename 原子替换） | `text_rev_changes_with_content`、`atomic_write_replaces_content`；源码模式保存传入版本号 |

## P2 — B 组：生命周期与状态身份

| # | 问题 | 修复 | 验证 |
| --- | --- | --- | --- |
| R07 | 管理接口轮询不绑进程，旧任务污染新状态 | 每次 spawn 分配递增 generation；`poll_admin`/`pump_logs` 请求前后都校验代次，失配即退出 | frpc.rs `admin_endpoint_parses_inline_and_auth` 等；代次门控为代码路径审查 |
| R08 | 旧进程 60s 计时器错误清零新进程重试预算 | 计时任务绑定 generation，仅本代次仍存活才清零 | 同上（`active_generation` 比对） |
| R09 | 指数退避底数写成 1 | `backoff_secs = 2^(n-1) capped 30` → 1,2,4,8,16,30 | `backoff_is_exponential_capped` |
| R10 | 子进程捕获无真实超时 | `run_hidden_capture` 管道读取移入线程 + `try_wait` 轮询截止时间，超时 kill 进程树返回 None；校验/探测命令改 async + `spawn_blocking` 移出主线程 | 代码路径审查（frpc -v 5s / verify 10s / where 5s 均为有效超时） |
| R15 | 新配置与旧运行状态按 name 混关联 | `last_saved_epoch` 与 `started_epoch` 派生 `pending_apply`；保存结果带 `applied`；仪表盘/代理页提示「已保存，重启后生效」 | 命令层 `mark_saved`/`fill_applied`；界面徽章 |
| R16 | 管理接口只认默认明文端点 | `AdminEndpoint` 解析 addr（含 0.0.0.0→127.0.0.1、IPv6 回环）/port/user/password，内联表可读；HTTP 请求带 Basic 认证并校验状态码（非 200 报错）；TLS 配置明确标注「观测不可用」并写日志 | `admin_endpoint_parses_inline_and_auth`、`admin_host_normalization`、`status_line_parsing` |
| R17 | 采样失败后旧数据无条件显示在线 | ProxyLive 增加 `stale`/`observed_at`；失败保留旧数据但标记，界面显示「未知」徽章 + 最后观测时间；恢复后自动刷新 | poll_admin Err 分支 + Dashboard/Proxies 徽章 |
| R18 | user 前缀导致运行名与配置名无法关联 | spawn 时解析 `user` 存入 `StatusSnap.user_prefix`；前端统一 `store.liveFor(name)` 做 `{user}.{name}` 映射，两个页面共用 | `user_prefix_extracted_from_doc` + store getter |
| R25 | 接管用启动时旧 PID 强杀、绕过停止协议 | 「结束全部」先刷新列表再确认；新命令 `frpc_kill_pid`：受管实例走 `stop_frpc`（设置 stop_requested 不触发自动重启），外部实例先复核仍是 frpc.exe 再杀（防 PID 复用误杀）；退出路径同样改为直接操作受管句柄 | commands.rs `frpc_kill_pid` + listeners.rs `verify_and_kill_frpc` |
| R26 | 停止/重启假报成功或永挂重连中 | `stop_internal` kill 后等待监督器回收（≤10s）才返回成功，超时/kill 失败如实报错；重启确认新 PID 后才报成功；自动重启 spawn 失败进入 stopped 终态并说明原因 | frpc.rs 停止/重启路径重构 |
| R27 | 停止态隐藏 last_error | 错误卡片独立于进程存活展示（「上次错误」）；停止态提供「重试启动」按钮 | Dashboard.vue |
| R28 | 先读快照后订阅，中间事件永久丢失 | boot 先注册监听再取快照；已有事件到达时不回填旧快照；日志按 id 去重；boot 失败有错误页与重试入口 | store.boot + App.vue |
| R31 | 业务状态依赖日志文本，日志写文件时永卡「启动中」 | 管理接口连续 3 次可达且无 running 代理时，从 starting 收敛为 running（connected 保持 false，界面显示「尚未确认登录」）；迟到成功日志仅在 starting/reconnecting/running 相位且代次匹配时生效 | pump_logs 相位白名单 + poll_admin empty_polls 逻辑 |

## P2 — C 组：编辑体验与系统集成

| # | 问题 | 修复 | 验证 |
| --- | --- | --- | --- |
| R05 | 4 个字段不参与 dirty 检测 | auth_method/transport_protocol/heartbeat_interval/heartbeat_timeout 全部纳入，数字统一 null 归一比较 | ConfigView.vue dirty computed |
| R11 | 运行时长不随秒更新 | computed 实际消费 `uptimeTick` 并由 `started_epoch` 现算 | Dashboard.vue |
| R20 | 切模式/离开页面静默丢草稿 | 源码模式保留未保存草稿（仅非 dirty 时重置）；切 tab 与路由离开均有确认对话框 | ConfigView.vue `switchTab`/`onBeforeRouteLeave` |
| R22 | 设置保存失败后两端保留新值 | 前端乐观更新失败回滚 + 提示；后端先落盘成功再提交内存；settings.json 损坏时备份原文件并以告警透出（不再静默当全新安装） | store.saveSettings + settings.rs load/save |
| R23 | 修改主题抹掉路径草稿 | watch 收窄为仅两个路径字段，且本地 dirty 时不回填 | SettingsView.vue |
| R24 | 自启动先存布尔再操作系统，失败不回滚 | 先 enable/disable 插件，再 `isEnabled()` 对账，按系统真实状态提交；失败不落库可重试 | SettingsView.vue toggle |
| R29 | 暂停不冻结内容；缓冲满后滚动失效 | 「冻结显示」语义：暂停即快照冻结视图；滚动由最后日志 id 触发（缓冲回绕也生效）；用户滚离底部不强行拽回 | LogsView.vue |
| R30 | 扫描任务无身份、订阅泄漏 | 事件携带 `task` 标识；工具页只接受本任务进度，`onBeforeUnmount` 回收 unlisten；日志页全局进度条兼容新格式 | Tools.vue + scan.rs |

## 观察项（Review §4，未计缺陷，本轮处置）

- mock 与演示副作用：bridge.js 改为仅浏览器模式动态加载 mock，桌面端不再执行演示定时器（§3.6）。
- HTTP 响应读取加 1MB 上限（§4.5 部分缓解）。
- 「接管」语义仍是「结束外部实例后由 OpenFrpc 拉起」，UI 文案已按实际行为描述（§4.7）。

## 未在本轮处理的边界（Review 已声明为需要产品决策）

- includes/visitors/模板等高级配置的建模与只读范围声明（§4.1/4.2）——界面目前以 warnings 提示解析限制。
- webServer 自定义 TLS 的观测支持（R16 的 Err 分支明确报「暂不支持」而非静默）。
- CSP 收紧与 webServer 自动注入的威胁建模（§4.6）。

## 复验指引

```bash
cd src-tauri && cargo test          # 30 项单元/回归测试
cd .. && node scripts/test-format.mjs  # 前端端口解析回归
npm run build                        # 前端构建
npx tauri build                      # NSIS 安装包
```

按 Review §5 的原生回归矩阵逐项手测仍建议在发布前执行（并发启停、外部改文件保存冲突、同名创建、HTTP↔TCP 迁移、自启动失败桩等）。
