// AI Usage Meter — the English and Chinese dictionaries and the helper that
// picks one. "Auto" resolves against the system locale.

import { settings, CLI, invoke, PREVIEW } from "./state.js";

const STRINGS = {
  en: {
    title: "AI Usage",
    viewRings: "Rings",
    viewDock: "Dock",
    dockEdge: "Tuck at screen edge",
    dockEdgeBtn: "Edge",
    dockColors: "Colors",
    dockRefresh: "Refresh",
    dockQuit: "Quit",
    dockConnected: "Connected",
    dockSignIn: "Sign in here",
    howToConnect: "How to connect",
    dockColorsNote: "Pick a swatch — the rings, the bars and the numbers follow",
    dockShortcutNote: "Global shortcut: open or tuck the Dock from anywhere",
    dockShortcutOn: "On — press it anywhere to open or tuck the Dock; click to turn off",
    dockShortcutOff: "Off — click to turn it on",
    dockReset: (d) => `resets in ${d}`,
    notConnectedShort: "Not connected",
    expiredShort: "Expired",
    offlineShort: "Offline",
    refresh: "Refresh (⌘R)",
    settings: "Settings",
    showAs: "Show numbers as",
    optLeft: "Left",
    optUsed: "Used",
    menuBar: "Menu bar",
    optIcon: "Icon",
    optPercent: "Icon + %",
    language: "Language",
    optAuto: "Auto",
    shortcut: "Shortcut",
    scOff: "Off",
    scCustom: "Custom",
    scCustomTitle: "Record your own combination",
    scPress: "Press keys…",
    scHint: "Press a combination — Esc to cancel",
    scNeedMod: (mac) => mac
      ? "Hold ⌘, ⌥, Ctrl or Alt with a key — or press F1–F12 on its own."
      : "Hold Ctrl, Alt or Shift with a key — or press F1–F12 on its own.",
    scTaken: "Another app already uses that shortcut. Pick the other one, or leave it off.",
    trendTitle: "Recent movement",
    etaDur: (h, m) => (h ? (m ? `${h}h ${m}m` : `${h}h`) : `${m}m`),
    eta: (d) => `~${d}`,
    etaTitle: (d) => `~${d} left`,
    launchAtLogin: "Launch at login",
    lowQuota: "Low-quota alerts",
    hideDisconnected: "Hide disconnected",
    edgeTuck: "Tuck at screen edge",
    colors: "Colors",
    resetAll: "Reset all",
    pick: "Pick a color",
    reset: "Back to default",
    expand: "Expand",
    star: "Star on GitHub",
    report: "Report an issue",
    quit: "Quit AI Usage Meter",
    privacy:
      "Reads the logins your official apps and CLIs already saved, or the accounts you sign in to here (the session stays inside this app). Read-only requests, straight to each service. No servers, no analytics.",
    local: "On-device · read-only",
    demo: "Demo data",
    left: "left",
    used: "used",
    tightest: "Tightest limit",
    busiest: "Most used",
    allClear: "All clear",
    resetsIn: (t) => `resets in ${t}`,
    resetsNow: "resetting now",
    noneTitle: "Nothing connected",
    noneNum: "Sign in to start",
    noneLine:
      "Sign in to Claude or Grok here, or use Claude Code, Codex, Grok Build, Gemini CLI or GitHub Copilot — then it just works.",
    loading: "Reading your quotas…",
    win: {
      session: "5-hour",
      weekly: "Weekly",
      weekly_opus: "Opus",
      weekly_sonnet: "Sonnet",
      gemini_pro: "Pro",
      gemini_flash: "Flash",
      premium: "Premium",
      chat: "Chat",
      period: "Period",
    },
    notConnected: (id) => {
      switch (id) {
        case "codex":
          return "Not connected. Open Codex in the ChatGPT desktop app and sign in, or run `codex login`.";
        case "gemini":
          return "Not connected. Sign in to Gemini CLI once (run `gemini`).";
        case "copilot":
          return "Not connected. Run `gh auth login`, or sign in to the Copilot CLI.";
        default:
          return `Not connected. Sign in to your account, or run \`${CLI[id]}\` once.`;
      }
    },
    webExpired: "Your sign-in expired.",
    signInRow: "Sign in",
    signIn: "Sign in",
    signOut: "Signed in",
    signOutTitle: "Click to sign out",
    acctClaude: "Claude account",
    acctClaudeSub: "claude.ai · same pool as the desktop app",
    acctGrok: "Grok account",
    acctGrokSub: "grok.com · same pool as the Grok app",
    fromSession: (t) => `from your latest Codex session · ${t}`,
    expired: (id) => {
      if (id === "gemini") return "Login expired. Run `gemini` once to refresh it.";
      if (id === "copilot") return "Login expired. Run `gh auth login` again.";
      return `Login expired. Run \`${CLI[id]}\` once to refresh it.`;
    },
    unavailable: (name) => `Couldn't reach ${name}. Retrying shortly.`,
    unavailableWhy: (name, why) => `Couldn't reach ${name} (${why}) — retrying shortly.`,
    // The service answered, but not in a shape this app knows: its API moved,
    // so signing in again or waiting will not help — the app needs updating.
    apiChanged: (name) =>
      `${name} answered in a format this app doesn't understand — its API has probably changed. Check for an app update.`,
    changedShort: "API changed",
    changedHint: "update the app",
    updateAvailable: (v) => `v${v} available`,
    checkUpdate: "Check for updates",
    checkUpdateBtn: "Check",
    upToDate: "You're on the latest version.",
    updateUnreachable: "Couldn't reach GitHub — try again later.",
    updateOpen: (v) => `Update to v${v}`,
    // The short reason a failed fetch puts in `note`, said in this language;
    // anything else (an `HTTP 502`, say) is already readable as it is.
    reason: {
      "timed out": "timed out",
      "offline or blocked": "offline or blocked",
      "network error": "network error",
      "unexpected response": "unexpected response",
    },
    scWayland: "Wayland doesn't let an app take a global shortcut. Sign in to an X11 session, or leave it off.",
    edgeWayland: "Wayland moves windows itself, so tucking at an edge is off here.",
    stale: "last reading",
    justNow: "just now",
    ago: (t) => `${t} ago`,
    dur: (d, h, m) => (d ? `${d}d ${h}h` : h ? `${h}h ${m}m` : `${Math.max(m, 1)}m`),
  },
  zh: {
    title: "AI 用量",
    viewRings: "环图",
    viewDock: "Dock",
    dockEdge: "贴边自动收起",
    dockEdgeBtn: "贴边",
    dockColors: "配色",
    dockRefresh: "刷新",
    dockQuit: "退出",
    dockConnected: "已连接",
    dockSignIn: "点此登录",
    howToConnect: "如何连接",
    dockColorsNote: "点色块自选，环、进度条和数字会立刻跟着变",
    dockShortcutNote: "全局快捷键：在任何地方按一下，就能打开或收起 Dock",
    dockShortcutOn: "已开启：在任何地方按一下就能打开或收起 Dock，点击关闭",
    dockShortcutOff: "已关闭：点击开启",
    dockReset: (d) => `${d} 后重置`,
    notConnectedShort: "未连接",
    expiredShort: "已过期",
    offlineShort: "连不上",
    refresh: "刷新 (⌘R)",
    settings: "设置",
    showAs: "数字显示为",
    optLeft: "剩余",
    optUsed: "已用",
    menuBar: "菜单栏",
    optIcon: "图标",
    optPercent: "图标 + %",
    language: "语言",
    optAuto: "自动",
    shortcut: "全局快捷键",
    scOff: "关闭",
    scCustom: "自定义",
    scCustomTitle: "录制自己的组合键",
    scPress: "请按组合键…",
    scHint: "按下组合键，Esc 取消",
    scNeedMod: (mac) => mac
      ? "按住 ⌘/⌥/Ctrl/Alt 再加一个键，或单按 F1–F12。"
      : "按住 Ctrl/Alt/Shift 再加一个键，或单按 F1–F12。",
    scTaken: "这个快捷键已被其他应用占用，换一个或选择关闭。",
    trendTitle: "最近用量走势",
    etaDur: (h, m) => (h ? (m ? `${h} 小时 ${m} 分` : `${h} 小时`) : `${m} 分钟`),
    eta: (d) => `约 ${d}`,
    etaTitle: (d) => `约 ${d}用完`,
    launchAtLogin: "开机启动",
    lowQuota: "低额度提醒",
    hideDisconnected: "隐藏未连接服务",
    edgeTuck: "贴边自动收起",
    colors: "服务配色",
    resetAll: "全部恢复",
    pick: "选择颜色",
    reset: "恢复默认",
    expand: "展开",
    star: "在 GitHub 上点个 Star",
    report: "反馈问题",
    quit: "退出 AI Usage Meter",
    privacy: "只读取官方应用和 CLI 已保存的登录，或你在这里登录的账号（登录状态只留在本应用里）。只发只读请求、直连各家服务。没有服务器，没有统计。",
    local: "本地运行 · 只读",
    demo: "演示数据",
    left: "剩余",
    used: "已用",
    tightest: "最紧张的额度",
    busiest: "用得最多",
    allClear: "额度充足",
    resetsIn: (t) => `${t}后重置`,
    resetsNow: "正在重置",
    noneTitle: "尚未连接",
    noneNum: "登录一个账号",
    noneLine:
      "在这里登录 Claude 或 Grok 账号，或用 Claude Code、Codex、Grok Build、Gemini CLI、GitHub Copilot，登录后自动显示。",
    loading: "正在读取额度…",
    win: {
      session: "5 小时",
      weekly: "本周",
      weekly_opus: "Opus",
      weekly_sonnet: "Sonnet",
      gemini_pro: "Pro",
      gemini_flash: "Flash",
      premium: "高级请求",
      chat: "对话",
      period: "周期",
    },
    notConnected: (id) => {
      switch (id) {
        case "codex":
          return "未连接。在 ChatGPT 桌面版里打开 Codex 并登录，或运行 `codex login`。";
        case "gemini":
          return "未连接。运行一次 `gemini` 登录 Gemini CLI。";
        case "copilot":
          return "未连接。运行 `gh auth login`，或登录 Copilot CLI。";
        default:
          return `未连接。登录账号，或运行一次 \`${CLI[id]}\`。`;
      }
    },
    webExpired: "登录已过期。",
    signInRow: "登录账号",
    signIn: "登录",
    signOut: "已登录",
    signOutTitle: "点击退出登录",
    acctClaude: "Claude 账号",
    acctClaudeSub: "claude.ai，与桌面版同一额度",
    acctGrok: "Grok 账号",
    acctGrokSub: "grok.com，与 Grok App 同一额度",
    fromSession: (t) => `来自最近一次 Codex 会话 · ${t}`,
    expired: (id) => {
      if (id === "gemini") return "登录已过期。运行一次 `gemini` 刷新登录。";
      if (id === "copilot") return "登录已过期。重新运行 `gh auth login`。";
      return `登录已过期。运行一次 \`${CLI[id]}\` 刷新登录。`;
    },
    unavailable: (name) => `暂时连不上 ${name}，稍后自动重试。`,
    unavailableWhy: (name, why) => `暂时连不上 ${name}（${why}），稍后自动重试。`,
    // 服务有回应，但返回的结构本应用已经读不懂：接口变了，重新登录或等待都没用，需要新版本。
    apiChanged: (name) =>
      `${name} 的返回格式变了，本应用暂时读不懂（服务可能改了接口）。看看有没有新版本。`,
    changedShort: "接口变了",
    changedHint: "需要更新应用",
    updateAvailable: (v) => `v${v} 可用`,
    checkUpdate: "检查更新",
    checkUpdateBtn: "检查",
    upToDate: "已是最新版本。",
    updateUnreachable: "暂时连不上 GitHub，稍后再试。",
    updateOpen: (v) => `更新到 v${v}`,
    reason: {
      "timed out": "超时",
      "offline or blocked": "离线或被拦截",
      "network error": "网络错误",
      "unexpected response": "返回格式异常",
    },
    scWayland: "Wayland 不允许应用占用全局快捷键，可以换到 X11 会话，或保持关闭。",
    edgeWayland: "Wayland 由系统自己摆放窗口，贴边收起在这里不可用。",
    stale: "上次读数",
    justNow: "刚刚更新",
    ago: (t) => `${t}前`,
    dur: (d, h, m) => (d ? `${d} 天 ${h} 小时` : h ? `${h} 小时 ${m} 分` : `${Math.max(m, 1)} 分钟`),
  },
};

function lang() {
  if (settings.lang !== "auto") return settings.lang;
  return (navigator.language || "en").toLowerCase().startsWith("zh") ? "zh" : "en";
}
const t = (key) => STRINGS[lang()][key];

// Rust writes the native low-quota notifications, so it needs the language
// this window resolved too — "auto" is settled right here, against the
// system locale, so the notification and the popover never disagree.
function syncLang() {
  if (PREVIEW) return;
  invoke("set_lang", { lang: lang() }).catch(() => {});
}

export { STRINGS, lang, t, syncLang };
