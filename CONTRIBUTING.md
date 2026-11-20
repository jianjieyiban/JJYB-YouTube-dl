# 参与 JJYB-YouTube-dl 开发

**源码版本：3.4.4（待发布）· 最新正式资产：3.4.3 · 修改日期：2026-11-20。** 本项目基于 [Open Video Downloader / youtube-dl-gui](https://github.com/jely2002/youtube-dl-gui)，使用 Vue 3、TypeScript、Tauri 2 与 Rust。保留上游及第三方版权，沿用 AGPL-3.0-or-later，不将衍生修改表述为完全原创。

本轮补充更新入口观察（原生 PID 9684）：当前非便携进程明确显示应用更新不可用，并保留自有仓库发布页链接；不能计作应用更新 busy/idle 或升级通过。单独点击工具更新后，按钮禁用并显示“请稍候…”，随后进入工具安装页；已观察到 ffmpeg 下载 79 MB / 79 MB、ffprobe 下载中。随后 UI 自动返回主页，两个已完成项仍保留。只读复核实际 managed bin（未借用 PATH）：ffmpeg 82797568 bytes、ffprobe 82668032 bytes，均实际报告 6.1.1；AtomicParsley 287232 bytes，报告 20240608.083822.0；三个绝对路径版本命令退出码均为 0，执行前后哈希一致，metadata 未锁定。主流程已读取系统 Temp `ovd-managed-tools-9684-20261120-131801.log`。此证据覆盖工具更新界面及落盘文件可执行，不代表后续任务实际工具选择、应用更新忙碌门禁、3.4.4 签名升级或回滚通过。

新增字幕嵌入实站验收（同一新 EXE、PID 9684）：新任务 `ae926780-11ad-4c84-ab91-1061e7a3e774` 选择人工英语、SRT、开启嵌入且关闭仅字幕，原生 UI 经初始化与字幕下载后完成 100%，队列已下载 3。实际 `(3).mp4` 为 971463 bytes，SHA256 `31500b7af982184f5c146f547c2da3796b939cc0b554864181e374252c37b41a`；managed ffprobe 退出 0，系统 Temp `jjyb-344-embed-native-ffprobe.json` 核实 19 秒、H.264 320×240、AAC、真实 `mov_text` 字幕流（18.881 秒）及 PNG 封面。匹配 `(3).en.srt` 为 416 bytes，主流程读取六段英文时间轴。旧两个 MP4/JPG 的哈希与修改时间保持不变。这仅通过成功嵌入，不代表嵌入失败回退或同名并发通过。此前从完成项偏好点击普通“下载”跳过已完成媒体、未生成新产物，不计作验收。

新增同名并发实站验收（同一新 EXE、PID 9684）：新组 `365247b9-522c-4ae7-ab47-1afb5c3e4dd7` 与 `cb75510e-7d5a-4882-acc3-74ce7da0075c` 均选择人工英语、SRT、开启嵌入且关闭仅字幕。一次队列“下载”后，原生同一观察中两个任务均为初始化，随后同一观察中均在下载字幕 100%，队列 3/5；最终两个任务均完成 100%，队列已下载 5。实际生成不同名尾缀的 `(4).mp4`、`(5).mp4`，各 971463 bytes，SHA256 均为 `31500b7af982184f5c146f547c2da3796b939cc0b554864181e374252c37b41a`；各自匹配的 `(4).en.srt`、`(5).en.srt` 各 416 bytes，SHA256 均为 `acbb5292c691dfe80af12aff4cd9f83a6654994d9ad6f07d96e195f38698fef7`，主流程已分别读取六段时间轴。两个 managed ffprobe 绝对路径命令均退出 0，系统 Temp `jjyb-344-concurrent-{4,5}-ffprobe.json` 核实 H.264、AAC、真实 mov_text 字幕流（18.881 秒）、PNG 封面及 19 秒容器。主流程读取 `jjyb-344-concurrent-before.txt` 与 `jjyb-344-concurrent-after.txt`，确认全部六个旧 MP4/JPG/SRT 大小、哈希与修改时间不变。本项通过同名并发与匹配字幕 sidecar、旧文件保护，不替代取消发布竞争、删除清理、嵌入失败回退或应用更新验收。

新增隔离便携原生验收（同一 `18bd…dccf` EXE、PID 28540）：在系统 Temp `jjyb-344-lifecycle-isolation` 使用独立配置、工具和输出，首次 metadata 报 `No usable yt-dlp found`；绝对路径 yt-dlp `--version` 退出 0、报告 2026.10.26，随后仅一次原生重交成功。初次失败组 `c9f8752b-969a-4cc1-8071-b34250f3c919` 保留，根因未确定，不能归因为超时或宣称修复。新组 `526853d8-5039-4a1b-b665-289702ec3bb9` 选择音频/MP3、人工英语 SRT、开启字幕嵌入、关闭仅字幕和封面嵌入；UI 完成 100%，本组日志错误 0、警告 1，明确提示 MP3 容器不支持嵌入并保留原媒体与独立 SRT。主流程实际运行隔离 ffprobe，系统 Temp `jjyb-344-fallback-ffprobe.txt` 核实 `outputs/Me at the zoo-(106k).mp3` 为 MP3 音轨、19.032 秒，并读取匹配 `.en.srt` 全部六段英文时间轴。这通过不支持容器的降级分支，不代表真实 FFmpeg 非零退出、取消发布顺序、启动可靠性或签名升级通过；队列仍保留初次失败项，汇总成功 1、失败 1，本轮未移除任务。

同一隔离便携进程空闲时点击应用“检查更新”，按钮禁用并显示检查中，随后 GitHub releases/latest 接口返回 HTTP 403。失败界面仍出现“更新并重启”控件，未点击任何更新或重启按钮，现场保留。本次只记录空闲检查失败，不计作 busy/idle 或升级通过，不重复盲试。

## 最新全面检查（2026-11-20）

修复应用更新检查失败仍显示“更新并重启”的问题：两个入口只在已确认有更新且无错误时展示该操作，错误时保留原因并提供“检查更新”，不显示重启承诺。新增单元回归先失败后通过，更新模块 15 项通过；浏览器下载/安装失败断言同步为重新检查入口。

本轮日志 `jjyb-344-audit-*` 位于系统 Temp：前端 277 项/36 文件、Rust 194 项、mock E2E 25 项、发布规则 13 项通过；lint、前端构建、全仓 rustfmt、严格 Clippy 通过。E2E 首次因旧用例仍期待错误后的重启按钮而失败，更新断言后以 2 workers 通过；误用不存在的 `release.test.mjs` 后，按实际 `scripts/release.test.ts` 成功执行，失败日志保留。第三方既有警告未屏蔽。原生重建和新版 UI 验收不由以上自动测试替代。

补充依赖检查：默认镜像审计接口返回 501，随后官方 npm 审计返回 31 个中危依赖节点（4 类 advisory）、无高危/严重，均为开发依赖；拒绝自动建议的不兼容降级和未经验证的跨主版本 overrides。兼容依赖升级已完成并回归：`vitest` 与 `@vitest/coverage-v8` 升至 4.1.12、`typescript-eslint` 与 `@typescript-eslint/parser` 升至 8.73.0（官方 registry、`--ignore-scripts`，日志 `jjyb-deps-resume-*`）；lint、277 项前端单测、双构建通过。npm 10.9.3 安装触发 Arborist 内部异常，改用 npm 11.21.3 完成；升级后官方审计降至 22 个中危依赖节点、无高危/严重，均为开发依赖（glob/minimatch 及许可工具传递链），未强行修复。`license-checker-rseidelsohn@5.0.1` 要求 Node ≥24 的 engine 警告保留。剩余 22 条为依赖传播节点，归并为两个根公告：glob 的取消遍历资源消耗（GHSA-3g3m-2fjr-6pw4）和 minimatch 在 optimizationLevel ≥2 处理恶意模式的资源消耗（GHSA-g3hv-75wg-m249）。当前 i18n lint 使用固定 `./src/locales/*.json` 同步 glob，未发现外部用户模式或 AbortSignal 进入该调用链；这不是零漏洞结论。兼容修复尚无证据，不强推跨主版本替换；CI 仅在可信输入下运行，避免向不可信 PR 暴露凭据。

更新链针对性核查：客户端与正式工作流的自有仓库地址、EXE 命名、清单与分离签名格式、SemVer 比较一致。远端只读查询确认最新正式 Release 为 v3.4.3，包含便携 EXE、ZIP、源码包、`portable-latest.json`、`.sig` 与 `SHA256SUMS`；仓库已配置应用/工具签名公私钥 Secret 名称（未读取密钥值，名称存在不代表配对与有效性已通过）。3.4.4 的源码推送不会自动成为可升级版本，需正式签名资产及发布验收；不得绕过验签或把该静态核查写成新版本升级通过。

新增工具发现修复（2026-11-20）：便携布局曾把普通用户数据目录的 managed `bin` 从候选列表中排除，空便携配置因此重复下载已有工具。`PathsManager` 现在独立保留 `managed_bin_dir`，候选顺序为 portable → EXE 同级 bin → 当前安装目录 → 普通 managed 目录 → PATH，保序去重；便携配置与安装落点不变，版本身份校验与主动更新分流保持独立。新增 2 项 paths 回归（首次因测试代码 E0382 编译失败，修正后通过）；全量 Rust 196 项、严格 Clippy、rustfmt 与新 `--no-bundle` 原生构建日志为系统 Temp `jjyb-344-dirsfix-*`。该修复针对重复 `/install` 下载症状，历史 `No usable yt-dlp found` 根因仍未知，不宣称超时修复。

Unix 实测前置检查：既有 WSL `JJYB-AnnaBuild` 可访问源码，但无 cargo/pkg-config；`JJYB-AnnaJammy` 启动失败，报告 `HCS/ERROR_PATH_NOT_FOUND`。未据此删除/重建用户发行版，也未将 Windows Rust 单测替代 Unix 进程终止验收。

原隔离 PID 28540 的窗口已失效，运行应用列表未找到该进程；未对其他实例发送操作，不据此推断崩溃。工具首次识别失败根因仍未确定，当前源码 probe 为 5 秒，不能将此前超时猜测写成修复结论。签名升级、取消发布竞争、跨平台真实进程终止等未通过项如实保留；当前 3.4.4 尚未打 tag、未发 Release，上述验收项与签名发布前置条件仍须补齐；不将源码推送表述为一键升级已发布。

新增工具文件替换修复：`atomic_file_replace` 不再先删除旧工具再 rename，而是直接使用文件系统替换，避免源文件缺失或替换失败时丢失仍可用的旧工具。新增“源缺失时保留旧内容”和“成功时替换内容并移除临时源”两项回归；第一项在修复前明确失败（旧文件 NotFound），修复后模块 2/2 通过，全仓 rustfmt 与 diff 检查通过。日志为系统 Temp `jjyb-atomic-file-replace-{red,green,fmt}.log`。这是此前全量 Rust 196 项及原生构建之后的独立小修复；随后按项目 `--all-features --all-targets` 执行最新后端回归，198 项通过、0 失败（系统 Temp `jjyb-344-close-rust.log`），包含两种取消/发布顺序、回执及暂停删除清理用例。定向静态核查未发现这些分支新的可证明错误；应用更新的 busy 门禁位于安装重启阶段，不禁止更新包下载。单测不替代原生取消竞争、删除和应用更新 busy/idle 验收。未将此前原生构建冒充包含此修复，也不将其归因为历史首次 yt-dlp 失败。

源码提交收尾：最新严格 Clippy（`--all-features --all-targets -- -D warnings`）通过，日志为系统 Temp `jjyb-344-close-clippy.log`；独立差异审阅未发现本轮引入的确定性正确性或隐私阻塞。已验证的修复可提交推送源码；尚未完成的原生发布验收继续阻止 3.4.4 tag/Release，不把源码推送当作正式签名升级已可用。构建、缓存和私人配置不纳入提交，保留用户产物与必要工具。

## 环境与构建

推荐 Node.js 24 LTS、npm、Rust stable。Windows 需要 MSVC、Visual Studio C++ Build Tools、Windows SDK 和 WebView2 Runtime；其他平台按 [Tauri 前置条件](https://v2.tauri.app/start/prerequisites/)准备，不代表本版已验证。

```powershell
npm ci
npm run tauri dev
npm run tauri build -- --no-bundle
```

最后一条仅构建主程序，本轮便携交付采用此方式。安装包需另行准备签名与打包环境；单独 `npm run dev` 不等于原生桌面应用。

必须保留 `src-tauri/vendor/stronghold-runtime/` 和 Cargo 的 `[patch.crates-io]` 本地覆盖，以及对应许可；不要删除 vendor 或改回注册表规避错误。`npm ci` 不会恢复 Rust vendor。保留锁文件，不随意更新依赖。

## 质量检查

在仓库根目录执行，不得降低门槛掩盖失败：

```powershell
npm run lint
npm run test:unit
npm run build
cargo fmt --manifest-path=src-tauri/Cargo.toml --all -- --check
npm run rust:clippy
npm run rust:test
npm run test:e2e
```

E2E 首次需安装 Playwright 浏览器及系统依赖。记录真实环境、命令、失败和复跑原因；检查完整仓库 rustfmt，不以局部检查替代。推送后核对 GitHub CI，不将等待中视为通过。

3.3.1 修复后前端单元测试 160 项、Rust 测试 155 项、类型检查、前端构建、全库格式和严格 Clippy 已通过；ESLint 0 错误、1 条中文原始文本警告。原生界面用本地 HTTP 样例验证视频 H.264/AAC MP4 和独立 JPG 封面。上一轮 Edge E2E 22/22 是历史结果，不能冒充本轮复跑。保留 Stronghold 及 libsodium PDB 第三方既有警告。

3.4.4 待发布源码将 SettingsUpdate 工具更新成功、失败提示改为 i18n，并新增 3 项回归测试覆盖默认英文、成功和失败提示。最新系统 Temp 记录与历史记录分开：

- `jjyb-344-ready-unit.log`：266 项、36 个测试文件通过；`jjyb-344-ready-lint.log` 通过。
- 主流程已成功执行 `jjyb-344-ready-e2e.log` 25 项与 `jjyb-344-ready-release.log` 13 项。
- 后续原生验收发现已完成媒体的封面按钮仍被禁用，单独修复组件条件，仅 configure/done 允许，普通下载不变。TDD 的 done 按钮断言先失败，再通过；目标测试 12 项通过。修复后单元测试 275 项、36 个文件通过（`--maxWorkers=2`；首次默认并发有 worker 启动超时），lint、类型检查、前端与隔离构建通过。主流程再次执行 mock E2E 25 项、发布规则测试 13 项通过。日志前缀为系统 Temp `jjyb-344-thumbnail-button-*`；这次组件修复的原生重建已通过（`jjyb-344-thumbnail-button-native.log`：`Finished release`、`Built application at`、`BUILD_EXIT_CODE=0`）；EXE SHA256 为 `18bd2d98a3a308aa71a8b291c48d9861865418d219aea5c090e1568cc2f7dccf`，11477504 bytes，当时完成项封面原生复验仍待补齐，后续 PID 9684 已取得下述新证据，不沿用旧 EXE 冒充通过。
- ready-native PID 24908 实站观察“初始化 → 暂停 → 继续 → 下载完成 100%”，队列已下载 1；新增 `Me at the zoo-(240p15) (2).mp4` 为 839292 bytes，实际 ffprobe 结果保存在系统 Temp `jjyb-344-pause-resume-ffprobe.json`。新旧 MP4 SHA256 均为 `ac6ee28203dbe767aac3d988afbb507b26451728d8f8e39c978b20a20a948d30`，旧文件修改时间保留。这仅覆盖初始化阶段暂停后继续成功，不等于取消/发布竞争、删除清理或同标题并发通过。
- 后续同一会话复查时，先前 JPG 的绝对路径由 Read 返回 NotFound，下载目录 glob 无匹配，Shell 的 USERPROFILE/TEMP 显示乱码。前述产物观察与 ffprobe 记录保留为历史，不推断文件被删除或环境变化，也不声称当前文件仍在；下一次原生验收须重新确认输出路径与真实文件。
- 新 EXE（上述 `18bd…dccf`）PID 9684 已重新核实实际下载目录和图片。新增组 `afa48a12-72e9-4e45-91eb-8c05a93c787d` 实站获取 `jNQXAC9IVRw`，关闭该任务字幕，普通视频下载完成 100%；再点同一完成项的“仅下载封面”，重新进入活动态并完成 100%，队列已下载 2。主流程实际读取新增 `Me at the zoo-(240p15) (2).jpg`，确认为 480×360 封面。前后只读快照确认两个 MP4 均为 839292 bytes，修改时间仍为 12:56:41 / 13:06:58，SHA256 均为 `ac6ee28203dbe767aac3d988afbb507b26451728d8f8e39c978b20a20a948d30`；两个 JPG 均为 23682 bytes，SHA256 均为 `f0003f468b2bfb100e88d2ead78bc0ab71872670c47945f1f0a19d58bbb538ae`。实际 ffprobe `(2).mp4` 为 19 秒、H.264 320×240 + AAC，完整证据在系统 Temp `openvideo-readonly-recheck-20261120-130925.txt`。这补齐新 EXE 完成媒体再次仅下载封面且原视频不变的验收，不等于生命周期竞争或升级通过。此前获准移除的初始化任务虽已从 UI 清空，后来观察到 12:56 MP4，不能据此断定发布顺序或后台清理完成。前述 NotFound 仅保留为历史观察，不推断根因。
- 此前 final 前端构建及 `jjyb-344-{fmt,clippy,rust}.log` 保留为历史：Rust 194 项、全库 rustfmt 与严格 Clippy 通过。保留测试及 Stronghold 等第三方既有警告，不将通过写为零警告。
- 此前 `jjyb-344-count-v2-native.log` Windows x64 原生构建通过；最新 `jjyb-344-ready-native.log` Windows x64 原生构建通过（第 160 行 `Finished release`、第 161 行 `Built application`，退出码 0），运行验收另算。保留 libsodium PDB 缺失的 LNK4099 等第三方既有警告。

本轮 3.4.3 原生真实外网测试 `jNQXAC9IVRw` 元数据成功，首次下载报 `No usable yt-dlp found`，手动重试成功；ffprobe 核验实际 MP4 为 629172 bytes、18.947483 秒、H.264 + AAC，无字幕产物，不等于字幕通过。单独工具 `--version` 连续 3 次（1534–2062 ms）及两并发（2007–2053 ms）均成功，首次原生失败根因未确定，不能猜测超时已修复。

3.4.4 原生 UI 已两次通过“仅下载字幕”取得 `jNQXAC9IVRw` 英文 SRT（416 bytes、6 段时间轴），包括已完成任务重交。普通媒体此前返回 HTTP 403，现保留为历史失败，不再表述为一直阻塞。最新 ready-native 3.4.4 原生启动 PID 10204，本轮 `jNQXAC9IVRw` 元数据成功、单独普通媒体下载成功；UI 下载完成 100%、队列已下载 1，无旧失败计数。“在文件夹中显示”定位 `C:\Users\李旺\Downloads\Me at the zoo-(240p15).mp4`。主流程已实际运行 ffprobe（退出码 0），系统 Temp `jjyb-344-ready-media-ffprobe.json` 读到 format duration `19.000000`、size `839292`，streams 为 H.264 320×240 + AAC + bin_data + PNG 480×360（封面），无 subtitle stream。此次媒体成功不等于字幕通过，此前两次 SRT 另计；单次成功不能证明网络稳定性。失败计数隔离及诊断清理修复此前通过 count-v2 单测 254 项与原生构建；最新 ready-native 原生构建和上述单条媒体运行结果分别记录，不宣称全面通过。下载失败不得增加 metadata processed/errored 计数；只清理重交项/组的旧诊断，保留元数据失败及已完成项。

同一 ready-native 已完成 media 组补充原生复验：Preferences 中仅字幕采用默认中文优先、未选语言时，明确报告无可下载字幕；metadata 只有德语/英语，符合策略，不是 bug。随后手动选择人工英语（中文优先自动关闭），再次仅字幕成功；主流程实际读取 `C:\Users\李旺\Downloads\Me at the zoo-(240p15).en.srt`，为 6 段英文时间轴，范围 1.2–18.881 秒。UI 完成 100%、队列已下载 1，不再残留刚才的 1 次失败。此为新增“失败 → 重交字幕成功、计数清除”原生验收，与此前两次 SRT 及媒体 ffprobe 证据分开记录。

历史中文翻译轨曾返回 HTTP 429，未取得该轨 SRT；此前原生测试 `BaW_jenozKc` 返回 `This video is unavailable`，也未取得字幕产物，不否定后续英文 SRT 两次通过。真实官方 3.4.2 → 3.4.3 更新成功链及限定故障就绪回退链已分别验证（见下文），不等于 3.4.4 升级通过。本地样例与模拟测试不能替代真实验收。3.4.4 尚未发布。最新普通媒体成功不消除网络稳定性风险，旧 `No usable yt-dlp found` 根因仍未知；3.4.4 升级（含其故障回退）尚未验收，剩余验收明确前阻止新版本打 tag/Release，不宣称全面验收。

新增下载回归修复：已完成媒体可提交独立字幕/封面任务；只有新的专项动作包含已完成项，普通下载与执行快照续传仍跳过已完成项。新增 3 项测试，已确认修复前两个专项测试失败、修复后该文件 27 项通过；该阶段 final 复跑单测 251 项、E2E 25 项、lint 和前端构建通过，仅为历史结果；最新 ready 结果见上文。专项动作在 IPC 前转换已完成项状态；普通下载与执行快照续传仍排除 completed。

## 回归与清理规则

详细约束见 [AGENTS.md](AGENTS.md)。重点包括：

- 播放列表 metadata 部分失败不提前 reject 自动下载，末项 fatal 仍可 finalize；全失败保留原组 error 并 reject，不删空组；等待已 error 组立即拒绝。
- 响应式下载选项可能包含嵌套 Proxy，按已知字段复制普通对象和数组，不直接 structuredClone；覆盖 IPC 前异常并反馈任务错误。
- 封面独立入口须贯穿前后端，跳过音视频，使用 after_video 封面清单；实际图片存在且有效才能判成功。
- 工具先在便携、同级 bin、托管目录与 PATH 中发现并校验身份；只安装缺失项，主动更新检查独立。
- UI 工具提示（包括工具更新成功、失败消息）必须使用 i18n，不硬编码中文；回归测试必须覆盖默认英文及成功、失败路径。
- 字幕默认值、单任务覆盖、重试状态和序列化保持一致；增加对应回归测试。
- 清理只移除已核对的旧版和测试残留，保留当前配置、下载文件、必要工具、vendor 与许可。禁止上传 Cookie、个人运行配置、WebView 缓存、诊断日志及密钥。

## 版本、便携产物与发布

当前 3.4.4 仅为待发布源码，最新正式资产目前为 3.4.3；3.4.3 已发布的 tag 不得覆盖或移动。同步 `package.json`、`package-lock.json` 顶层及根包、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock` 自身包和 `src-tauri/tauri.conf.json` 的版本，不全局替换历史版本或第三方依赖版本。更新 README、使用文档及许可中本项目版本；重新构建后复制程序，再计算 SHA256，不只改目录名冒充新版本。

当前范围是 Windows x64 便携版，没有安装器和其他平台已验收产物。公开包必须从白名单资源生成，不能直接压缩个人运行目录；需要附对应源码、vendor、许可证和修改说明。自有仓库为 [jianjieyiban/JJYB-YouTube-dl](https://github.com/jianjieyiban/JJYB-YouTube-dl)，公开资产以实际 Release 为准。仅推送源码不等于已发布 Release。

便携应用更新仅面向自有仓库最新正式稳定 Release 的 `portable-latest.json` 与 `portable-latest.json.sig`，使用内置可信公钥验证 Ed25519 签名。3.4.3 已发布签名清单；不得回退上游、跳过验签或伪造未签名清单。正式便携发行工作流需要配置对应签名环境，不能以旧 `latest.json` 安装器机制替代。签名工具清单 Pages 需显式启用 `JJYB_ENABLE_SIGNED_MANIFEST` 并配置签名环境，与应用更新独立。真实隔离系统 Temp 中，官方 3.4.2 经 UI“检查更新 → 更新并重启”到 3.4.3 成功：journal `phaseCommitted`、`old_version=3.4.2`、`old_hash=bbfa958d5da20d26e92e7aa795919940881be95df37fc5bc73ed93009fc5445a`，app-ready nonce 匹配；升级后 EXE SHA256 `dd4e96d67ea49bb94421c9d9d289b2e15d0d53edeafc93323ac9dc535d7c4e18` 与官方 manifest 3.4.3 一致，“关于”显示 3.4.3。中文界面、MP4、字幕启用仍可见，但未做逐字段配置比较。只确认 3.4.2 → 3.4.3 成功链，不等于 3.4.4 升级通过；另一次限定故障就绪回退验证见下段。更新清单签名不等于 Windows Authenticode。

用户另行授权了隔离系统 Temp 内官方 3.4.2 → 3.4.3 的故障回退验证：watcher 独占预置普通、非 nonce 的 `app-ready` 文件，不修改官方 EXE 或签名。事务 `.jjyb-update-13831d655c884cc083b4f55ed4acc3bd` 实际经历 `Staged → Replacing → AwaitingReady → RollingBack → RolledBack`，系统 Temp 日志 `jjyb-344-rollback-watch.log` 记录后台退出码 0。恢复的 target SHA256 `bbfa958d5da20d26e92e7aa795919940881be95df37fc5bc73ed93009fc5445a` 与官方 3.4.2 一致；旧版自动重启 PID 18988，“关于”实际显示 3.4.2。已验证的仅是 3.4.2 → 3.4.3 故障就绪回退链，不等于 3.4.4 升级验收。

## 隐私、提交与许可

前端仅在非开发、非 E2E 且配置 `VITE_JJYB_SENTRY_DSN` 时初始化遥测；Rust 独立读取 `JJYB_SENTRY_DSN`。默认均不设置，不恢复上游 DSN。启用后不能保证错误上下文完全脱敏；前端 Vite 变量不能存秘密。

采用 `fix: ...`、`docs: ...`、`chore: ...` 等清晰提交信息，描述目的、测试与未验证范围。只暂存相关文件，不跳过 hooks，不强推。翻译需保持键、占位符与 HTML 标记结构，检查前后端回退。

保留 [LICENSE](LICENSE)、[第三方声明](licenses/3rdpartylicenses.txt) 及 OpenCC 随附许可；分发修改版须提供对应源码。软件按许可证“按原样”提供，不作担保。
