# JJYB-YouTube-dl

基于 [Open Video Downloader / youtube-dl-gui](https://github.com/jely2002/youtube-dl-gui) 的衍生桌面下载工具，采用 Vue 3、TypeScript、Tauri 2 与 Rust，通过 yt-dlp 下载媒体及平台字幕，使用 FFmpeg 后处理。

**版本：3.3.0 · 修改日期：2026-11-16。** 本项目并非完全原创，保留上游与第三方版权及许可。

## 下载与使用

- [自有仓库](https://github.com/jianjieyiban/JJYB-YouTube-dl) · [Release 下载](https://github.com/jianjieyiban/JJYB-YouTube-dl/releases)
- 本次发行范围为 Windows x64 便携版；未提供安装器及其他平台已验证产物。具体资产以 Release 页面为准。
- 解压完整压缩包至可写目录，保留 `jjyb-portable` 与随附资源，启动 `JJYB-YouTube-dl.exe`。需要 Microsoft Edge WebView2 Runtime。
- [中文使用文档](docs/使用文档.md) · [构建与开发](CONTRIBUTING.md) · [隐私说明](docs/privacy.html)

## 3.3.0 变更与来源

**上游原本已经支持字幕下载、语言选轨、自动字幕及字幕嵌入。源站存在可用简体中文字幕时，可以选择相应语言下载；这些不是本衍生版首创。** 上游基线 `c402ee3` 已包含字幕设置、单任务人工/自动轨选择与 yt-dlp 字幕参数实现。

本衍生版在此基础上提供：

- 中文优先策略（`chinesePriority`）：已有简体 → 繁体本地转简 → 平台提供的中文翻译轨，同层优先人工字幕；可额外选择英、日、韩语。
- 仅字幕模式，不下载音视频；默认独立 SRT，普通视频可同时嵌入。
- 修复字幕 ID 大小写、精确选轨、带来源后缀的中文翻译轨匹配，以及暂停恢复/重试时设置丢失。
- 修复普通视频无字幕时的完成状态；仅字幕无实际输出不能判成功。
- 衍生品牌、中文文档、便携资源与自有更新端点；默认不启用遥测，未配置更新公钥时禁用应用更新。
- 更新兼容范围内的 brace-expansion 锁定版本，CI Node 统一为 24。

仅提取平台已有字幕或翻译轨，不做无字幕语音识别，不调用外部机器翻译；繁简转换不是翻译。视频、音频、播放列表、封面和元数据等基础能力沿用上游及 yt-dlp/FFmpeg。

## 验证结果与已知限制

- 前端单元测试 152/152、Rust 测试 146/146、系统 Edge E2E 22/22；ESLint、类型检查、前端构建、Clippy 严格检查及 Windows Tauri `--no-bundle` 构建通过。
- Edge 曾出现加载超时，最终按原测试、单 worker、无并行编译复跑通过；未修改用例或增加重试掩盖，先前根因未证实。
- 保留 Stronghold 的两条第三方既有警告与 libsodium PDB 链接警告。3.3.0 发布提交曾因 5 个 Rust 文件未按 rustfmt 格式化导致 CI 失败，现已统一格式；此修复不改变程序逻辑，既有 Release 二进制保持不变。
- npm 官方审计生产依赖 0 漏洞；开发依赖仍有 33 项 moderate，无 high，不代表完整应用或外部工具无风险。
- 真实 YouTube 已解析并选中中文翻译轨，但字幕下载遇到 HTTP 429，未取得 SRT，**真实站点全链路未验收**。不保证所有站点、账号、地区可用，不绕过平台验证或 TLS。
- **应用自动更新禁用，签名升级链未验收；本次便携版手动更新。** 未提供未签名的 `latest.json` 冒充安全更新。程序未作 Windows Authenticode 签名，系统可能提示未知发布者。
- 公开包只从白名单资源生成，不包含本地运行配置、Cookie、WebView 缓存或诊断日志；外部工具不承诺为最新版本。SHA256 以 Release 随附校验文件为准。

## 从源码构建

推荐 Node.js 24 LTS、npm、Rust stable。Windows 使用 MSVC、Visual Studio C++ Build Tools、Windows SDK 与 WebView2。Node 22 至少 22.13，并须满足锁定依赖要求。其他平台按 [Tauri 前置条件](https://v2.tauri.app/start/prerequisites/)准备，不代表本版已验证。

```powershell
npm ci
npm run tauri dev
# 仅构建主程序（本次便携发行方式）
npm run tauri build -- --no-bundle
```

完整源码必须保留 `src-tauri/vendor/stronghold-runtime/` 和 Cargo 的 `[patch.crates-io]` 路径覆盖；不要删除 vendor 或改回注册表来规避错误。安装产物构建使用 `npm run tauri build`，需另行准备签名和平台打包环境。

## 更新、隐私与许可

应用更新目标仅为本仓库 `releases/latest/download/latest.json`；公钥为空时禁用，不回退上游、不跳过验签。Tauri 更新签名不同于 Windows Authenticode。未来至少用两个真实签名 Release 验证完整升级链，私钥只放受保护的 CI Secrets。

yt-dlp/FFmpeg 组件更新与应用升级独立。便携升级先退出并备份必要配置，只替换 Release 指定程序与资源，不覆盖下载文件。

默认不启用 Sentry，未内置上游 DSN。前端仅在非开发、非 E2E 且配置 `VITE_JJYB_SENTRY_DSN` 时初始化；Rust 后端独立读取 `JJYB_SENTRY_DSN`。启用后不能保证错误上下文全面脱敏。默认关闭遥测不等于离线，下载、封面及工具更新仍访问网络。

沿用 [AGPL-3.0-or-later](LICENSE)。感谢 Jelle Glebbeek 及上游贡献者；第三方声明见 [许可汇总](licenses/3rdpartylicenses.txt)。分发修改版须提供对应源码、修改标记及适用许可，不以更名免除义务。

仅下载拥有权利或已获授权的内容，遵守法律与平台条款。软件按许可证“按原样”提供，不作担保。反馈前移除 Cookie、私有链接、账号与代理口令。
