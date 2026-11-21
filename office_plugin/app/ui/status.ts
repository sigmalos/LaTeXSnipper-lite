// What the Setup window says about `latexsnipper-office-host --status`, and
// how it explains the host's error codes. No DOM, so it is unit tested.

export type Lang = "zh" | "en";

export interface HostStatus {
  version: string;
  installed: boolean;
  running: boolean;
  reachable: boolean;
  certificate: { exists: boolean; trusted: boolean };
  hosts: { id: "word" | "powerpoint"; app: boolean; ready: boolean; sideloaded: boolean }[];
  model: { source: "desktop" | "addin" | null; downloadBytes: number };
  location: string | null;
  url: string;
}

export interface Row {
  label: string;
  value: string;
  level: "ok" | "warn" | "bad" | "off";
  action?: "download";
}

const STRINGS = {
  zh: {
    intro: "在 Word 和 PowerPoint for Mac 中插入、编辑可再次修改的 LaTeX 公式。安装后插件随登录自动运行，不需要打开 LaTeXSnipper 桌面端。",
    install: "安装",
    repair: "安装 / 修复",
    uninstall: "卸载",
    confirmUninstall: "再点一次确认卸载",
    help: "使用说明",
    download: "下载",
    service: "插件服务",
    serviceRunning: "运行中",
    serviceNotInstalled: "未安装",
    serviceDown: "已安装，但没有响应，请点击“安装 / 修复”",
    certificate: "本地证书",
    certificateTrusted: "已信任（仅用于 localhost）",
    certificateUntrusted: "未信任，安装时 macOS 会请求密码",
    hostReady: "已就绪，在“开始 › 加载项”中选择 LaTeXSnipper",
    hostAfterInstall: "安装后可用",
    hostPending: "插件服务会在一分钟内加载",
    hostNotOpened: "请先打开一次 {app}，再回来安装",
    hostMissing: "未安装 {app}",
    model: "截图识别模型",
    modelDesktop: "使用 LaTeXSnipper 已下载的模型",
    modelAddin: "已下载",
    modelMissing: "未下载（{size} MB）",
    installing: "正在安装…macOS 会请求密码以信任本地证书。",
    installed: "安装完成。重新打开 Word 或 PowerPoint 后，在“开始 › 加载项”中选择 LaTeXSnipper。",
    uninstalling: "正在卸载…macOS 可能会请求密码以移除证书。",
    uninstalled: "已卸载。Office 中的插件、证书信任、设置和插件下载的模型都已移除。",
    downloadStarting: "正在连接下载服务器…",
    downloading: "正在下载公式识别模型：{done} / {total} MB",
    downloaded: "模型已下载，可以使用截图识别了。",
    trustDenied: "没有信任本地证书（已取消或密码不正确），Office 无法加载插件页面。请重试。",
    locationDmg: "请先把 LaTeXSnipper Office 拖到“应用程序”文件夹，再从那里打开并安装。",
    locationTranslocated: "macOS 正从临时位置运行本应用。请把它移到“应用程序”文件夹后重新打开；若仍提示，在终端执行 xattr -dr com.apple.quarantine \"/Applications/LaTeXSnipper Office.app\"。",
    portInUse: "端口 28766 已被其他程序占用，插件服务无法启动。",
    agentFailed: "无法注册登录项：{detail}",
    hostMissingError: "应用不完整，请重新下载 LaTeXSnipper Office。",
    downloadFailed: "下载失败：{detail}",
    failed: "操作失败：{detail}",
  },
  en: {
    intro: "Insert and edit re-editable LaTeX formulas in Word and PowerPoint for Mac. Once installed, the add-in runs from login; the LaTeXSnipper desktop app is not needed.",
    install: "Install",
    repair: "Install / Repair",
    uninstall: "Uninstall",
    confirmUninstall: "Click again to uninstall",
    help: "Help",
    download: "Download",
    service: "Add-in service",
    serviceRunning: "Running",
    serviceNotInstalled: "Not installed",
    serviceDown: "Installed but not responding. Click Install / Repair.",
    certificate: "Certificate",
    certificateTrusted: "Trusted (localhost only)",
    certificateUntrusted: "Not trusted; macOS asks for your password during installation",
    hostReady: "Ready. Choose Home › Add-ins › LaTeXSnipper.",
    hostAfterInstall: "Available after installation",
    hostPending: "The add-in service loads it within a minute",
    hostNotOpened: "Open {app} once, then install",
    hostMissing: "{app} is not installed",
    model: "Recognition model",
    modelDesktop: "Using the model downloaded by LaTeXSnipper",
    modelAddin: "Downloaded",
    modelMissing: "Not downloaded ({size} MB)",
    installing: "Installing… macOS asks for your password to trust the local certificate.",
    installed: "Installed. Reopen Word or PowerPoint and choose Home › Add-ins › LaTeXSnipper.",
    uninstalling: "Uninstalling… macOS may ask for your password to remove the certificate.",
    uninstalled: "Uninstalled. The add-in, certificate trust, settings and downloaded model were removed.",
    downloadStarting: "Connecting to the download server…",
    downloading: "Downloading the formula model: {done} / {total} MB",
    downloaded: "Model downloaded. Screenshot recognition is ready.",
    trustDenied: "The local certificate was not trusted (cancelled or wrong password), so Office cannot load the add-in. Try again.",
    locationDmg: "Drag LaTeXSnipper Office to the Applications folder first, then open it from there to install.",
    locationTranslocated: "macOS is running this app from a temporary location. Move it to Applications and reopen it; if this persists, run xattr -dr com.apple.quarantine \"/Applications/LaTeXSnipper Office.app\" in Terminal.",
    portInUse: "Port 28766 is used by another program, so the add-in service cannot start.",
    agentFailed: "Could not register the login item: {detail}",
    hostMissingError: "The app is incomplete. Download LaTeXSnipper Office again.",
    downloadFailed: "Download failed: {detail}",
    failed: "Failed: {detail}",
  },
} as const;

export type Key = keyof (typeof STRINGS)["zh"];

export function text(lang: Lang, key: Key, vars: Record<string, string> = {}): string {
  return STRINGS[lang][key].replace(/\{(\w+)\}/g, (_, k: string) => vars[k] ?? `{${k}}`);
}

export function isChinese(locale: string | undefined): boolean {
  return (locale ?? "").toLowerCase().startsWith("zh");
}

const APP_NAMES = { word: "Word", powerpoint: "PowerPoint" } as const;

export function describeStatus(lang: Lang, s: HostStatus): Row[] {
  const t = (k: Key, v?: Record<string, string>) => text(lang, k, v);
  const service: Row = !s.installed
    ? { label: t("service"), value: t("serviceNotInstalled"), level: "off" }
    : s.reachable
      ? { label: t("service"), value: t("serviceRunning"), level: "ok" }
      : { label: t("service"), value: t("serviceDown"), level: "bad" };
  const certificate: Row = s.certificate.trusted
    ? { label: t("certificate"), value: t("certificateTrusted"), level: "ok" }
    : { label: t("certificate"), value: t("certificateUntrusted"), level: s.installed ? "bad" : "off" };
  const hosts: Row[] = s.hosts.map((h) => {
    const app = APP_NAMES[h.id];
    if (!h.app && !h.ready) return { label: app, value: t("hostMissing", { app }), level: "off" };
    if (!h.ready) return { label: app, value: t("hostNotOpened", { app }), level: "warn" };
    if (!s.installed) return { label: app, value: t("hostAfterInstall"), level: "off" };
    return h.sideloaded && s.reachable
      ? { label: app, value: t("hostReady"), level: "ok" }
      : { label: app, value: t("hostPending"), level: "warn" };
  });
  const size = String(Math.round(s.model.downloadBytes / 1e6));
  const model: Row =
    s.model.source === "desktop"
      ? { label: t("model"), value: t("modelDesktop"), level: "ok" }
      : s.model.source === "addin"
        ? { label: t("model"), value: t("modelAddin"), level: "ok" }
        : { label: t("model"), value: t("modelMissing", { size }), level: "warn", action: "download" };
  return [service, certificate, ...hosts, model];
}

/** The host prints `CODE:detail` on its first stderr line. */
export function describeError(lang: Lang, raw: string): string {
  const t = (k: Key, v?: Record<string, string>) => text(lang, k, v);
  const message = raw.replace(/^Error:\s*/, "").trim();
  const [code, ...rest] = message.split(":");
  const detail = rest.join(":").trim() || message;
  switch (code) {
    case "TRUST_DENIED":
      return t("trustDenied");
    case "LOCATION_DMG":
      return t("locationDmg");
    case "LOCATION_TRANSLOCATED":
      return t("locationTranslocated");
    case "PORT_IN_USE":
      return t("portInUse");
    case "AGENT_FAILED":
      return t("agentFailed", { detail });
    case "HOST_MISSING":
      return t("hostMissingError");
    case "DOWNLOAD_FAILED":
      return t("downloadFailed", { detail });
    default:
      return t("failed", { detail: message });
  }
}
