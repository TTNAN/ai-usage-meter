<div align="center">

<img src="assets/icons/app-icon-512.png" width="112" alt="AI Usage Meter" />

# AI Usage Meter

**Claude、ChatGPT（Codex）、Grok、Gemini、GitHub Copilot 的订阅额度，常驻菜单栏，一眼看清。**

同一个应用，两种形态：**环图弹窗**看全局，**紧凑 Dock 条**只占屏幕一角——随时切换，设置和配色两边通用。

中文 · [English](./README_EN.md)

[![Build](https://github.com/TTNAN/ai-usage-meter/actions/workflows/build.yml/badge.svg)](https://github.com/TTNAN/ai-usage-meter/actions/workflows/build.yml)
[![Release](https://img.shields.io/github/v/release/TTNAN/ai-usage-meter?include_prereleases&color=6e9bff)](https://github.com/TTNAN/ai-usage-meter/releases)
![Platforms](https://img.shields.io/badge/macOS%20%7C%20Windows-111?logo=tauri&logoColor=white)
[![License](https://img.shields.io/badge/license-MIT-b38cff)](./LICENSE)

<picture>
  <source media="(prefers-color-scheme: light)" srcset="docs/hero-light.png" />
  <img src="docs/hero.png" alt="AI Usage Meter 菜单栏弹窗（环图视图）" width="880" />
</picture>

<picture>
  <source media="(prefers-color-scheme: light)" srcset="docs/dock-light.png" />
  <img src="docs/dock.png" alt="紧凑 Dock 视图：一服务一格" width="520" />
</picture>

</div>

写代码写到一半被「额度用完」打断？AI Usage Meter 把五家的 5 小时窗口、周额度、模型额度和重置倒计时放进菜单栏，**不用打开网页、不用粘贴 Cookie、不经过任何服务器**。

## 亮点

- **两种形态，一个应用** — 底栏一行「环图 / Dock」随时换：环图弹窗把每个窗口、每条倒计时摊开；Dock 条只有 496×138（五个服务时），一格一个服务——官方图标、大号百分比和一条细进度条，悬停才显示重置时间与预计用完，平时不占地方。格子会跟着显示的服务数收窄，三家就只占 304px。
- **一眼看清最紧张的那一个** — 同心环总览 + 大号数字，直接告诉你哪家额度最先见底、多久后重置。连上几家就画几环：3 家是经典三环，4–5 家会自动加密环数、收细描边，一家都不落下。
- **菜单栏图标本身就是仪表** — 三根小电量条实时显示额度最紧的三个服务（16px 的图标放三根最清楚）；macOS 上还可以在图标旁直接显示百分比。
- **每个窗口都看得到** — Claude 的 5 小时 / 本周（以及 Opus、Sonnet 单独额度）、ChatGPT 里 Codex 的 5 小时 / 本周、Grok 的计费周期、Gemini 的 Pro / Flash 额度、Copilot 的高级请求和对话额度，各自带倒计时。
- **额度在往哪走也看得见** — 每行记一小段本机历史，用一条 36×12 的迷你折线画出走势；按最近的速度估算「约多久用完」，只在真的会在重置前来得及用完时才写出来，安静的额度不多占一行。
- **全局快捷键** — 默认 `⌘⇧U` / `Ctrl+Shift+U`，在任何应用里都能唤出或收起弹窗；设置里可换成 `⌘⇧A` 或关掉，被别的应用占用时会如实说明。
- **订阅档位** — 自动识别 Max、Pro、Plus、SuperGrok、Ultra、Individual 等档位。
- **零配置** — 直接复用 ChatGPT 桌面版（Codex）、Claude Code、Grok Build、Gemini CLI / Antigravity、GitHub CLI / Copilot CLI 已有的登录；只用 Claude / Grok 网页版或 App？在设置里登录一次账号就行。Gemini 的访问令牌一小时就过期，应用会拿 refresh token 自己续上并写回 `~/.gemini/oauth_creds.json`，不用再跑一次 `gemini`。
- **实时跟着变** — 后台每 1 到 5 分钟自动重读，你在网页、App、命令行哪里用掉的额度都会同步显示。
- **原生质感** — macOS 原生毛玻璃弹窗、跟随系统深浅色、点击图标弹出、失焦自动收起；Windows 上从托盘弹出。
- **拖到哪就在哪，可贴边** — 按住标题栏或空白处可以拖到屏幕任意位置；拖到左／右边缘松手就吸附上去，失焦时平滑滑出屏幕，只留一条 12px 的小把手，点它（或点菜单栏图标）再滑回来。设置里的「贴边自动收起」可以关掉。
- **自定义服务配色** — 「设置 → 服务配色」一行一个服务：点色块用系统取色器自选，或点旁边的预设色，同心环、进度条和强调色立刻跟着变；Windows 的托盘小条也会用同一套颜色。点 ↺ 或「全部恢复」回到各服务原本的配色。
- **中英双语** — 跟随系统语言，也可在设置里切换。
- **安静可靠** — 后台低频轮询、手动刷新有限速；单次请求失败会保留上次读数并标注，绝不编造数字。
- **轻** — Tauri v2 + 原生 HTML/CSS/JS，没有 Node 依赖，没有前端打包器。

<p align="center"><img src="docs/states.png" alt="中文界面、服务配色、首次启动和浅色模式" width="880" /></p>

## 安装

从 [Releases](https://github.com/TTNAN/ai-usage-meter/releases) 下载：

| 平台 | 文件 | 说明 |
| --- | --- | --- |
| macOS（Apple 芯片 + Intel） | `.dmg` | 拖进「应用程序」。尚未公证，首次打开请右键 → 打开，或运行下方命令 |
| Windows 10 / 11 | `.exe` 或 `.msi` | 双击安装，无需管理员权限 |

```bash
xattr -cr "/Applications/AI Usage Meter.app"
```

> [!TIP]
> 应用尚未公证，所以首次打开被 macOS 拦下时，右键 → 打开，或者运行上面这行命令即可；之后就能正常双击启动了。

然后登录你在用的工具（用哪个登哪个，没登录的会显示「未连接」）：

| 服务 | 最简单的方式 | 也可以 |
| --- | --- | --- |
| Claude | 点 Claude 一行的「登录账号」（或 设置 → Claude 账号），登录 claude.ai | 已装 [Claude Code](https://docs.anthropic.com/en/docs/claude-code) 的话自动读取 |
| ChatGPT（Codex 额度） | 在 [ChatGPT 桌面版](https://openai.com/chatgpt/download/) 里打开 Codex 并登录 | [Codex CLI](https://github.com/openai/codex) `codex login` |
| Grok | 点 Grok 一行的「登录账号」（或 设置 → Grok 账号），登录 grok.com | 已装 [Grok Build](https://docs.x.ai/build/overview) 的话自动读取 |
| Gemini | 登录一次 [Gemini CLI](https://github.com/google-gemini/gemini-cli)（运行 `gemini`） | 用 Google Antigravity 登录过的账号同样自动读取 |
| GitHub Copilot | 运行 `gh auth login`（GitHub CLI） | 已装 Copilot CLI 并用它登录过的话自动读取 |

> Claude 网页版、桌面版、手机版和 Claude Code 共用同一份额度；Grok 网页版、App 和 Grok Build 也共用同一个每周额度池。所以登录任意一处，看到的都是同一个数字。登录窗口里如果 Google 登录被拦截，换邮箱登录即可。
>
> Gemini 用 `gemini-api-key` 或 Vertex AI 方式登录时没有可查的额度接口，这一行会说明原因，改用 Google 账号登录即可；Copilot 读取的是 GitHub 账号的 Copilot 订阅（个人版、商业版都行），订阅失效时会提示续订。

## 快捷键

| 按键 | 作用 |
| --- | --- |
| `⌘⇧U` / `Ctrl+Shift+U` | 全局快捷键：在任何应用里唤出或收起弹窗（设置里可换 `⌘⇧A` 或关掉） |
| `⌘R` / `Ctrl R` | 刷新 |
| `⌘,` | 打开设置 |
| `Esc` | 收起弹窗（贴边时收回屏幕边缘） |
| `⌘Q` / `Ctrl Q` | 退出 |

## 工作原理

这几家目前都没有公开的「订阅额度」API，所以 AI Usage Meter 读取各家官方应用自己在用的接口：要么复用官方 CLI / 桌面版已保存在本机的登录，要么用你在应用里登录的网页账号。

| 服务 | 接口 | 凭证位置 | 轮询 |
| --- | --- | --- | --- |
| Claude | `GET api.anthropic.com/api/oauth/usage` | macOS 钥匙串 `Claude Code-credentials`，或 `~/.claude/.credentials.json` | 5 分钟 |
| ChatGPT（Codex） | `GET chatgpt.com/backend-api/wham/usage`；读不到登录或请求失败时，改读 `~/.codex/sessions` 里最近一次会话记录的额度 | `~/.codex/auth.json`（ChatGPT 桌面版与 Codex CLI 共用，支持 `$CODEX_HOME`） | 1 分钟 |
| Grok | `GET cli-chat-proxy.grok.com/v1/billing` | `~/.grok/auth.json`（支持 `$GROK_HOME`） | 5 分钟 |
| Gemini | `POST cloudcode-pa.googleapis.com/v1internal:loadCodeAssist` 取档位，`:retrieveUserQuota` 取每个模型的剩余额度；令牌过期时先 `POST oauth2.googleapis.com/token` 续期再重试一次 | `~/.gemini/oauth_creds.json`（支持 `$GEMINI_HOME`，Gemini CLI / Antigravity 共用） | 5 分钟 |
| GitHub Copilot | `GET api.github.com/copilot_internal/user` | GitHub OAuth Token：`$GH_TOKEN` / `$GITHUB_TOKEN`、`gh` 的 `hosts.yml`、Copilot CLI 的 `hosts.json`，最后回退到 `gh auth token` | 5 分钟 |
| Claude 网页账号 | `GET claude.ai/api/organizations/{org}/usage` | 应用内登录窗口的 claude.ai 会话 | 3 分钟 |
| Grok 网页账号 | grok.com 用量页自己请求的 `GetGrokCreditsConfig` | 应用内登录窗口的 grok.com 会话 | 5 分钟 |

网页账号的请求在一个隐藏的窗口里、在对方网站自己的页面内发出（和你在浏览器里打开用量页一样），结果交回应用后显示。会话 cookie 只留在应用自己的浏览器存储里，不会被读出、复制或发往别处；在设置里点「已登录」即可退出并清除。

> [!NOTE]
> 2026 年 7 月起 Codex 桌面版并入了 ChatGPT 桌面版，所以这一行叫 ChatGPT。它显示的是 **Codex 的用量额度**，不是 ChatGPT 聊天的额度，两者分开计算。
>
> Claude 和 Grok 两行，CLI 登录和网页账号都能用，两者都有时优先用 CLI，读不到再用网页账号。

## 隐私

- 凭证只在发请求的那一刻从本机读取，只发给它所属的那家服务，不落盘、不上传。
- 只发查询用量的请求（Gemini 用的是官方 CLI 自己的两个 POST 查询接口），不会改动你的账号或 CLI 配置文件。唯一会写回本机的，是 Gemini 令牌续期后 `~/.gemini/oauth_creds.json` 里的 `access_token` 和 `expiry_date` 两个字段，其余内容原样保留。
- Gemini 续期用的是 Gemini CLI 自己的 OAuth 客户端，它不属于这个项目，因此本仓库不收录：续期时从你本机已安装的 `@google/gemini-cli` 包里读取（`oauth_creds.json` 自带时就先用它自己的），也可以用 `GEMINI_CLI_OAUTH_CLIENT_ID` / `GEMINI_CLI_OAUTH_CLIENT_SECRET` 指定。找不到就只显示「请重新登录」，不会去猜。
- 用量走势只存在本应用自己的 `localStorage` 里，不经过网络。
- 没有服务器、没有统计、没有遥测。代码就这几千行，欢迎审阅：[`crates/ai-usage-core/src/providers.rs`](src-tauri/crates/ai-usage-core/src/providers.rs)、[`web.rs`](src-tauri/src/web.rs)。

## 风险说明

这些都是非官方 / 内部接口，随时可能变化，届时对应一行会显示「暂时连不上」，而不是错误的数字；以各家官方设置页为准。低频只读轮询的风险较低，目前没有因此封号的公开案例，但我们无法保证。

## 参与开发

不装 Rust 也能改界面：用任意静态服务器打开 `src/`，页面会自动进入预览模式并使用演示数据。

```bash
python3 -m http.server -d src 8000
# http://localhost:8000/                  默认：五家演示数据（含走势与预计用完）
# http://localhost:8000/?view=dock        紧凑 Dock 视图（五个格子）
# http://localhost:8000/?view=dock&state=three&hide=1   三家服务的 Dock 条
# http://localhost:8000/?state=trend      同上，重点看迷你走势与「约多久用完」
# http://localhost:8000/?state=empty       首次启动
# http://localhost:8000/?state=mixed       混合状态（未连接 / 登录过期）
# http://localhost:8000/?state=four        只连四家（四环总览）
# http://localhost:8000/?view=settings     设置页（含服务配色与全局快捷键）
# 加 &lang=zh 或 &lang=en 可强制界面语言
```

构建桌面应用需要 Rust stable：

```bash
cargo install tauri-cli --version "^2"
cargo tauri dev                    # 开发
cargo tauri build                  # 打包
AI_USAGE_METER_MOCK=1 cargo tauri dev   # 用演示数据运行
```

后端分成两部分：应用自己的 `src-tauri/src/`（托盘、窗口、命令、轮询、网页登录），和不依赖 Tauri 的核心库 `src-tauri/crates/ai-usage-core/`（五家 provider、低额度提醒规则、托盘图标、贴边吸附）。两个视图共用同一套后端；窗口尺寸也由页面量好上报（`set_window_size`），所以环图和 Dock 条能共用一个原生窗口。

```bash
cd src-tauri/crates/ai-usage-core && cargo test   # 核心库单元测试
cd src-tauri && cargo test                        # 应用层（需要 Tauri 的系统依赖）
```

想支持更多服务（Cursor、Windsurf…）？欢迎提 [Issue](https://github.com/TTNAN/ai-usage-meter/issues) 或 PR：每个服务只是 `ai-usage-core/src/providers.rs` 里的一个函数。

## 路线图

- [x] 额度即将用尽时的系统通知
- [x] 更多服务：Gemini CLI / Antigravity、GitHub Copilot
- [x] 用量走势迷你折线与「预计多久用完」
- [x] 全局快捷键（`⌘⇧U` / `Ctrl+Shift+U`）
- [x] 把 Dock 版并进同一个应用：环图 / Dock 双形态（0.7.0）
- [ ] 更多服务：Cursor、Windsurf
- [ ] Linux 托盘
- [ ] macOS 签名与公证

## 致谢

- [@Litx4i](https://x.com/Litx4i/status/2123400444458488080) — 灵感来源
- [Quota](https://github.com/josephbergvinson/quota) — 复用 CLI 登录状态的思路

如果它帮你省下了一次「额度用完」的打断，欢迎点个 ⭐️，这是让它继续变好的最大动力。

## 请我喝杯咖啡

如果这个项目帮你省了点时间，欢迎请我喝杯咖啡。☕

| 微信赞赏码 |
| -------- |
| <img src="assets/wechat-pay.png" width="220" alt="微信赞赏码"> |

## 许可证与商标

[MIT](./LICENSE)。Claude 是 Anthropic 的商标；OpenAI / ChatGPT / Codex 是 OpenAI 的商标；Grok / xAI 是 xAI 的商标；Gemini / Google 是 Google 的商标；GitHub Copilot 是 GitHub 的商标。品牌图标仅用于标识对应服务，不代表任何背书：Gemini 用的是 Google 官方四色星芒（Wikimedia Commons `Google_Gemini_icon_2025.svg`），Copilot 用的是 GitHub Primer Octicons 的 `copilot-24.svg`，其余见图标来源说明。截图使用演示数据。
