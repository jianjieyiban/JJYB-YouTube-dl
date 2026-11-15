# 参与 JJYB-YouTube-dl 开发

本项目是 [Open Video Downloader / youtube-dl-gui](https://github.com/jely2002/youtube-dl-gui) 的衍生版，使用 Vue 3、TypeScript、Tauri 2 与 Rust。修改说明更新于 **2026-11-15**；保留上游作者与第三方许可，不将衍生修改表述为完全原创。

自有仓库入口为 [jianjieyiban/JJYB-YouTube-dl](https://github.com/jianjieyiban/JJYB-YouTube-dl)。截至本次文档更新，仓库已创建但尚空、尚无 Release；本地文件不代表已推送。本指南不承诺上游分支策略、CI 或商店发布流程已经适用于本仓库。

## 本地环境

- 推荐 Node.js 24 LTS 与 npm；如使用 Node 22，至少使用 22.13，并确认满足当前锁定依赖的要求。项目尚未在 `package.json` 统一声明 `engines`，不能只按 Vite 的最低版本选 Node。
- Rust stable、Cargo；Windows 选择 MSVC 工具链。
- Windows 安装 Visual Studio Build Tools 的“使用 C++ 的桌面开发”、Windows SDK 与 Microsoft Edge WebView2 Runtime。
- macOS/Linux 按 [Tauri 前置条件](https://v2.tauri.app/start/prerequisites/)安装系统依赖；未完成本衍生版跨平台验收。

从获授权的完整源码开始，在项目根目录执行：

```powershell
node --version
npm --version
rustc --version
cargo --version
npm ci
npm run tauri dev
```

`npm ci` 使用 `package-lock.json`，不要为绕过问题随意重写锁文件。单独运行 `npm run dev` 只是前端开发服务，不等同可操作系统、下载或更新的 Tauri 桌面应用。

### 必须保留的本地 Stronghold 修复

完整源码必须包含 `src-tauri/vendor/stronghold-runtime/`，且 `src-tauri/Cargo.toml` 保留：

```toml
[patch.crates-io]
stronghold-runtime = { path = "vendor/stronghold-runtime" }
```

不要删除该目录、修改依赖源码或回退注册表版本来规避构建错误。缺失时补齐对应源码；`npm ci` 不会恢复 Rust vendor 内容。分发源码也应包含该修复及其许可。

## 常用命令

以下命令均在项目根目录执行，具体脚本以 `package.json` 为准：

| 命令 | 用途 |
| --- | --- |
| `npm run lint` | 检查前端代码规范 |
| `npm run lint:fix` | 自动修复规范问题；执行后检查是否产生无关变更 |
| `npm run test:unit` | Vitest 单元测试 |
| `npx playwright install` | 首次安装 E2E 所需浏览器；Linux 还需系统依赖 |
| `npm run test:e2e` | Playwright 测试 |
| `npm run build` | 类型检查并构建前端与隔离页，不生成桌面安装包 |
| `npm run rust:fmt` | 格式化 Rust 代码，会修改文件 |
| `npm run rust:clippy` | Rust 静态检查 |
| `npm run rust:test` | Rust 测试 |
| `npm run tauri build` | 构建桌面程序及适用安装产物 |

产物通常位于 `src-tauri/target/release/bundle/`，具体以实际构建输出为准。Tauri 配置启用了更新产物生成，正式发布需要准备签名；缺少签名或平台打包工具不应通过跳过验签来“解决”。记录实际 Node、npm、Rust 版本、执行命令、通过项与失败项，不把前端构建成功等同完整桌面验收。

### 本轮已报告的核验结果

核验环境为 Node.js 22.20.0 / npm 10.9.3；146 项前端单元测试、132 项 Rust 测试、22 项系统 Edge E2E、4 项本地真实媒体嵌入/取消验证、ESLint 与生产构建通过。Windows Tauri `--no-bundle` 构建成功，最终 exe 已交付并核对 SHA256；原生更新配置提示已验证。此次未生成安装器，仍有依赖 unused/dead_code 和 libsodium PDB 链接警告。npm 官方审计生产依赖为 0 漏洞；包含开发依赖仍有 34 项（33 moderate、1 high），不是全依赖清零，也不覆盖 Rust 或外部工具。

本地交付目录为 `JJYB便携版-3.2.1`，运行组件沿用旧包的 6 个文件，不含配置或 Cookie，不承诺工具版本为最新。不要将组件复用表述为完成了最新版本升级或真实下载验收。

## 代码与测试范围

- 前端位于 `src/`，Rust 后端位于 `src-tauri/src/`；沿用现有 Vue、TypeScript 和 Rust 风格。
- 字幕设置与选轨逻辑分别包含前端配置和后端执行，改动时保持字段、默认值与序列化一致。
- 为行为修改增加对应测试，特别关注字幕优先级、单任务覆盖、繁简转换、仅字幕、更新未配置与错误路径。
- E2E 中的模拟下载和更新不等同真实 YouTube 或真实签名 Release 验收。**真实 YouTube 下载与字幕全链路、两个签名 Release 间的升级链尚未验证**，不得写为已通过。

## 遥测与敏感数据

默认未配置 DSN 时不初始化 Sentry，不再内置上游 DSN：

- 前端：`src/sentry.ts` 读取构建时的 `VITE_JJYB_SENTRY_DSN`；空值、开发或 E2E 模式跳过初始化。Vite 变量会进入前端构建产物，不能放私钥、口令或其他秘密。
- 后端：`src-tauri/src/lib.rs` 仅在启动时环境变量 `JJYB_SENTRY_DSN` 非空时初始化，不因前端处于开发模式而自动跳过。
- 两端独立，当前错误采样率为 0.25、性能采样率为 0.05；配置 DSN 后可能发送错误与上下文，不保证全面脱敏。

构建默认关闭遥测的发行版时，不设置前端 DSN，且运行环境不设置后端 DSN。不要恢复上游 DSN。提交问题和测试材料前移除 Cookie、账号、私有链接、代理凭据及个人路径，详见[隐私说明](docs/privacy.html)。

## 应用更新与发布

应用更新固定目标是自有仓库的 `releases/latest/download/latest.json`，不回退上游。当前 `src-tauri/tauri.conf.json` 的 `plugins.updater.pubkey` 为空，应用更新被禁用；组件更新是独立链路。

发布前应配置与发布私钥配对的有效 Tauri 公钥、生成带签名的各平台更新产物及匹配版本/架构的 `latest.json`。仅填非空字符串不代表密钥有效或更新链已验收。私钥和口令只能通过受保护的 CI Secrets 注入，不得提交、写入日志或打包。Tauri 更新签名不同于 Windows Authenticode 签名。

至少用两个真实签名 Release 验证旧版检查、下载、验签、安装和新版启动，并检查错误签名、网络失败与未配置状态。便携版按实际 Release 说明手动替换，不承诺自动原地升级。不要在验证前宣传发布流水线或在线升级可用。

## 提交与翻译

使用独立主题分支和清晰的 `feat: ...`、`fix: ...`、`docs: ...` 等提交信息；PR 说明变更目的、测试结果及未验证范围。待仓库实际启用协作后，按维护者公布的目标分支与检查规则提交，不直接套用上游发布分支约定。

翻译涉及 `src/locales/<语言代码>.json` 和 `src-tauri/locales/<语言代码>.json` 两处。不要改键名、JSON 结构、占位符（如 `{count}`、`{error}`）、技术名称或 HTML 标记结构。添加语言时同时检查前后端注册与回退行为。

## 许可

贡献使用项目的 [AGPL-3.0-or-later](LICENSE)。保留 Jelle Glebbeek 等上游贡献者的版权，以及 [第三方许可汇总](licenses/3rdpartylicenses.txt)、OpenCC 等附带声明。分发修改版应按适用许可提供对应源码和修改说明。
