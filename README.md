# JJYB-YouTube-dl

基于 [Open Video Downloader / youtube-dl-gui](https://github.com/jely2002/youtube-dl-gui) 的衍生桌面下载工具，采用 Vue 3、TypeScript、Tauri 2 和 Rust，通过 [yt-dlp](https://github.com/yt-dlp/yt-dlp) 获取视频、音频及平台字幕，并使用 [FFmpeg](https://ffmpeg.org/) 处理媒体。

**修改标记：2026-11-15。** 本项目并非完全原创，保留上游及第三方版权、许可和来源说明。

## 当前状态与入口

- 自有仓库：[jianjieyiban/JJYB-YouTube-dl](https://github.com/jianjieyiban/JJYB-YouTube-dl)。截至本次文档更新，仓库已创建但尚空，尚无 Release；本地修改不代表已经上传。
- 当前为本地便携交付版本。核验环境为 Node.js 22.20.0 / npm 10.9.3；146 项前端单元测试、132 项 Rust 测试、22 项系统 Edge E2E、ESLint 和生产构建通过。Windows Tauri `--no-bundle` 构建成功，最终 exe 已复制并校验 SHA256；原生界面确认未配置更新公钥时禁用应用更新。不代表安装器、真实 YouTube 下载链或签名 Release 更新链已验证。
- npm 官方审计：生产依赖 0 漏洞；包含开发依赖共 34 项（33 moderate、1 high），未清零。此结果不覆盖 Rust、外部工具或完整应用安全性。
- 本地交付目录名为 `JJYB便携版-3.2.1`，运行组件沿用旧包的 6 个文件，不包含配置或 Cookie，不承诺这些工具为最新版本；该目录不等于已公开发布或已验收的安装包。
- 不提供现成公开安装包承诺，也不声称 Windows、macOS、Linux 均已构建或测试通过。
- [中文使用文档](docs/使用文档.md) · [本地构建说明](CONTRIBUTING.md) · [隐私说明](docs/privacy.html)

## 功能与边界

- 视频、音频、播放列表及下载队列；可设置分辨率、容器、保存目录和文件名。
- 输出设置包含封面嵌入、封面独立保存与媒体元数据；可用性受格式、站点和 FFmpeg 支持限制。
- 支持代理、Cookie 文件和浏览器 Cookie 选项；能否成功访问取决于站点权限和运行环境。
- 本轮字幕功能：中文优先策略（`chinesePriority`）按 **已有简体 → 繁体本地转简 → YouTube 提供的中文翻译轨** 尝试，同一层优先人工字幕。可额外选择英语、日语、韩语。
- 默认保存独立 SRT；普通视频可选同时嵌入。任务字幕页有“仅字幕（不下载音视频）”选项。单任务明确选轨优先于中文策略。
- **仅提取平台提供的字幕或翻译轨**：没有字幕不会语音识别，也不调用外部机器翻译；繁简转换不是翻译。本轮构建、自动测试及 4 项本地真实媒体嵌入/取消验证已通过，真实 YouTube 场景仍待验收。

## 安装与启动

目前请从获授权的完整本地源码构建；自有仓库有源码或 Release 后，再按其实际内容获取。不要将上游安装包、商店条目、Homebrew 或 WinGet 包当成本衍生版。

未来若发布 Windows 安装版，下载匹配系统架构的安装包并按向导安装。便携交付如有提供，应保留随附文件并在可写目录运行；单独复制一个 exe 不保证形成完整便携包。Windows 运行环境需要 WebView2。具体包名、支持系统与验证结果以届时 Release 为准。

### 从源码构建

准备：

- **推荐 Node.js 24 LTS**，以及 npm；如使用 Node 22，至少使用 22.13，并确认满足当前锁定依赖的要求。`package.json` 未设置统一 `engines`；不能仅按 Vite 的最低版本选择环境。执行构建时记录实际 Node、npm 和 Rust 版本，不以环境要求代替构建验证结果。
- Rust stable 与 Cargo；Windows 使用 MSVC 工具链。
- Windows 的 Visual Studio Build Tools，勾选“使用 C++ 的桌面开发”及相应 Windows SDK；安装 Microsoft Edge WebView2 Runtime。
- macOS/Linux 需另行满足 [Tauri 官方平台前置条件](https://v2.tauri.app/start/prerequisites/)，本项目暂不提供跨平台验证保证。

在源码根目录运行：

```powershell
node --version
npm --version
rustc --version
npm ci
npm run tauri dev
```

构建生产安装产物：

```powershell
npm run tauri build
```

通常在 `src-tauri/target/release/bundle/` 查看实际生成的安装包；生成哪些平台产物取决于主机与配置。`npm run build` 仅构建前端，不能代替 Tauri 桌面打包。

**保留本地 Stronghold 修复：** 完整源码必须包含 `src-tauri/vendor/stronghold-runtime/`，并保留 `src-tauri/Cargo.toml` 的 `[patch.crates-io]` 本地路径覆盖。不要删除 vendor 或改回注册表依赖来规避构建错误；缺失时应补齐完整源码。`npm ci` 不会补齐这份 Rust 源码。

## 更新与发布安全

应用更新与 yt-dlp/FFmpeg 等组件更新是两条不同链路，组件更新开关不代表应用更新已可用。

本衍生版的应用更新目标仅为：

```text
https://github.com/jianjieyiban/JJYB-YouTube-dl/releases/latest/download/latest.json
```

发布前必须完成自有端点、与私钥配对的 Tauri 签名公钥及签名产物配置；**未配置有效安全公钥时默认禁用应用更新**，不得回退上游、使用空公钥或跳过验签。当前尚无 Release，自有在线更新未验收，不能将配置目标理解为已有可用服务。

签名私钥和口令只能通过受保护的 CI Secrets 注入，不得提交到源码、日志或安装包。应用内只分发公钥。Tauri 更新签名不等同 Windows Authenticode 签名，不保证消除系统安全提示。至少需要两个真实签名 Release，才能验证旧版检查、下载、验签、安装及新版启动的完整更新过程。

安装版的应用更新依赖安装器和更新产物；便携版不承诺自动原地升级，默认按 Release 说明手动替换，先退出程序、备份必要配置，不覆盖下载文件。更多见[使用文档的更新说明](docs/使用文档.md#updates)。本次文档工作不提交、不推送、不创建 Release。

## 隐私

**默认不启用 Sentry 遥测，源码不再内置上游 DSN。** 前端仅在构建时配置非空 `VITE_JJYB_SENTRY_DSN` 且非开发、非 E2E 模式时初始化；Rust 后端仅在启动时读取到非空环境变量 `JJYB_SENTRY_DSN` 时初始化，两者独立。前端开发模式不会自动禁用已配置 DSN 的后端。启用后可能发送错误、诊断上下文及抽样性能数据；不能保证日志和错误内容已经全面脱敏。

默认关闭遥测不等于离线：解析与下载、远程封面、组件更新以及配置后的应用更新仍会访问相关服务。详情见[隐私说明](docs/privacy.html)。报告问题前请移除 Cookie、账号、私有链接、代理密码及日志中的个人信息。

## 许可、来源与使用责任

本项目沿用 [AGPL-3.0-or-later](LICENSE)。上游为 Jelle Glebbeek 等贡献者维护的 [Open Video Downloader / youtube-dl-gui](https://github.com/jely2002/youtube-dl-gui)，本次修改包括衍生品牌、中文使用说明及相关开发整合，不抹除原作者贡献。

第三方组件、字体、图标及运行时各自的许可仍适用，见 [第三方许可汇总](licenses/3rdpartylicenses.txt) 和各组件随附声明。分发修改版时应按适用许可提供对应源码、修改说明与必要版权声明，不以更名免除义务。

仅下载你拥有权利或已获授权的内容，遵守当地法律和平台条款。维护者不鼓励规避访问限制或侵权使用；软件按许可证规定“按原样”提供，不作担保，责任限制见许可证第 15、16 条。
