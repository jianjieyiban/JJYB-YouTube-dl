# 参与 JJYB-YouTube-dl 开发

**版本：3.4.3 · 修改日期：2026-11-19。** 本项目基于 [Open Video Downloader / youtube-dl-gui](https://github.com/jely2002/youtube-dl-gui)，使用 Vue 3、TypeScript、Tauri 2 与 Rust。保留上游及第三方版权，沿用 AGPL-3.0-or-later，不将衍生修改表述为完全原创。

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

真实 YouTube 字幕曾返回 HTTP 429，未取得 SRT；真实外网全链路和签名升级链仍未验收。本地样例与模拟测试不是这些场景的替代证据。

## 回归与清理规则

详细约束见 [AGENTS.md](AGENTS.md)。重点包括：

- 响应式下载选项可能包含嵌套 Proxy，按已知字段复制普通对象和数组，不直接 structuredClone；覆盖 IPC 前异常并反馈任务错误。
- 封面独立入口须贯穿前后端，跳过音视频，使用 after_video 封面清单；实际图片存在且有效才能判成功。
- 工具先在便携、同级 bin、托管目录与 PATH 中发现并校验身份；只安装缺失项，主动更新检查独立。
- 字幕默认值、单任务覆盖、重试状态和序列化保持一致；增加对应回归测试。
- 清理只移除已核对的旧版和测试残留，保留当前配置、下载文件、必要工具、vendor 与许可。禁止上传 Cookie、个人运行配置、WebView 缓存、诊断日志及密钥。

## 版本、便携产物与发布

同步 `package.json`、`package-lock.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock` 和 `src-tauri/tauri.conf.json`。更新 README、使用文档及许可中本项目版本；重新构建后复制程序，再计算 SHA256，不只改目录名冒充新版本。

当前范围是 Windows x64 便携版，没有安装器和其他平台已验收产物。公开包必须从白名单资源生成，不能直接压缩个人运行目录；需要附对应源码、vendor、许可证和修改说明。自有仓库为 [jianjieyiban/JJYB-YouTube-dl](https://github.com/jianjieyiban/JJYB-YouTube-dl)，公开资产以实际 Release 为准。仅推送源码不等于已发布 Release。

便携应用更新仅面向自有仓库最新正式稳定 Release 的 `portable-latest.json` 与 `portable-latest.json.sig`，使用内置可信公钥验证 Ed25519 签名。3.4.3 已发布签名清单；不得回退上游、跳过验签或伪造未签名清单。正式便携发行工作流需要配置对应签名环境，不能以旧 `latest.json` 安装器机制替代。签名工具清单 Pages 需显式启用 `JJYB_ENABLE_SIGNED_MANIFEST` 并配置签名环境，与应用更新独立。真实跨版本升级全链路仍需两个正式签名 Release 验证。更新清单签名不等于 Windows Authenticode。

## 隐私、提交与许可

前端仅在非开发、非 E2E 且配置 `VITE_JJYB_SENTRY_DSN` 时初始化遥测；Rust 独立读取 `JJYB_SENTRY_DSN`。默认均不设置，不恢复上游 DSN。启用后不能保证错误上下文完全脱敏；前端 Vite 变量不能存秘密。

采用 `fix: ...`、`docs: ...`、`chore: ...` 等清晰提交信息，描述目的、测试与未验证范围。只暂存相关文件，不跳过 hooks，不强推。翻译需保持键、占位符与 HTML 标记结构，检查前后端回退。

保留 [LICENSE](LICENSE)、[第三方声明](licenses/3rdpartylicenses.txt) 及 OpenCC 随附许可；分发修改版须提供对应源码。软件按许可证“按原样”提供，不作担保。
