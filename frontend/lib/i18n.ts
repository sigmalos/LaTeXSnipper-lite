// Strings are keyed by their Chinese source text, like the original app's tr().
export type Lang = "zh" | "en";

const EN: Record<string, string> = {
  "截图识别": "Capture",
  "快速入门": "Quick start",
  "历史记录": "History",
  "最新在前": "Newest first",
  "最早在前": "Oldest first",
  "清空": "Clear",
  "快捷键": "Shortcut",
  "收藏夹": "Favorites",
  "设置": "Settings",
  "LaTeX 编辑器": "LaTeX editor",
  "图片识别": "Image",
  "PDF 识别": "PDF",
  "手写识别": "Handwriting",
  "复制": "Copy",
  "导出": "Export",
  "实时渲染预览": "Live preview",
  "清空预览": "Clear preview",
  "在此输入 LaTeX 公式，下方将实时渲染...": "Type LaTeX here; it renders below as you type…",
  "当前模型": "Model",
  "状态": "Status",
  "就绪": "Ready",
  "需下载模型": "Models needed",
  "识别中…": "Recognizing…",
  "外部模型": "External model",
  "公式": "Formula",
  "混合": "Mixed",
  "纯文字": "Text",
  "还没有识别记录。按 {hotkey} 截图识别，或把图片拖到窗口里。":
    "No recognitions yet. Press {hotkey} to capture, or drop an image on the window.",
  "还没有识别记录。拖入图片或粘贴截图即可识别。":
    "No recognitions yet. Drop an image or paste a screenshot to recognize it.",
  "收藏夹是空的。在历史记录的菜单里选择“收藏”。": "No favorites yet. Choose “Favorite” from a history item’s menu.",
  "已复制": "Copied",
  "已复制 {format}": "Copied {format}",
  "复制到剪贴板": "Copy to clipboard",
  "重命名": "Rename",
  "收藏": "Favorite",
  "删除": "Delete",
  "载入编辑器": "Load into editor",
  "已收藏": "Added to favorites",
  "已在收藏夹中": "Already in favorites",
  "识别结果": "Recognition result",
  "确认或修改结果：": "Review or edit the result:",
  "确认并复制": "Confirm and copy",
  "取消": "Cancel",
  "关闭": "Close",
  "保存": "Save",
  "已确认并复制到剪贴板": "Confirmed and copied",
  "未检测到可识别内容": "Nothing recognizable was found",
  "识别失败：{error}": "Recognition failed: {error}",
  "低置信度，请检查结果": "Low confidence. Check the result.",
  "确定要清空全部 {count} 条历史记录吗？": "Clear all {count} history items?",
  "确定要清空全部 {count} 条收藏吗？": "Clear all {count} favorites?",
  "清空全部": "Clear all",
  "名称": "Name",
  "下载模型": "Download models",
  "{mode}识别需要下载 MathCraft 模型（约 {size}），只需下载一次。":
    "{mode} recognition needs the MathCraft models (about {size}). They download once.",
  "开始下载": "Download",
  "正在下载 {model}": "Downloading {model}",
  "模型已就绪": "Models are ready",
  "下载失败：{error}": "Download failed: {error}",
  "识别后端": "Recognition engine",
  "内置 MathCraft（离线）": "Built-in MathCraft (offline)",
  "外部模型（OpenAI 兼容接口）": "External model (OpenAI-compatible API)",
  "识别类型": "Recognition type",
  "外观主题": "Appearance",
  "跟随系统": "System",
  "浅色": "Light",
  "深色": "Dark",
  "界面语言": "Language",
  "PDF 渲染分辨率": "PDF render resolution",
  "接口地址": "Base URL",
  "模型名称": "Model name",
  "API Key（本地保存，仅当前用户可读）": "API key (stored locally, readable only by you)",
  "超时（秒）": "Timeout (seconds)",
  "自定义提示词（留空使用内置模板）": "Custom prompt (leave empty for the built-in one)",
  "测试连接": "Test connection",
  "连接成功：{message}": "Connected: {message}",
  "连接失败：{error}": "Connection failed: {error}",
  "设置已保存": "Settings saved",
  "数据目录": "Data folder",
  "全局截图快捷键": "Global capture shortcut",
  "按下新的组合键…": "Press a new key combination…",
  "组合键需要至少一个修饰键（⌘ ⌥ ⌃ ⇧）": "Use at least one modifier (⌘ ⌥ ⌃ ⇧)",
  "恢复默认": "Reset to default",
  "快捷键已更新为 {hotkey}": "Shortcut changed to {hotkey}",
  "快捷键不可用：{error}": "Shortcut unavailable: {error}",
  "导出为 {format}": "Export as {format}",
  "复制为": "Copy as",
  "已导出到 {path}": "Exported to {path}",
  "导出失败：{error}": "Export failed: {error}",
  "内容为空": "Nothing to export",
  "Word（需要 Pandoc）": "Word (needs Pandoc)",
  "书写": "Pen",
  "橡皮": "Eraser",
  "撤销": "Undo",
  "清空画布": "Clear canvas",
  "插入编辑器": "Insert into editor",
  "在此书写，停笔后自动识别": "Write here. Recognition runs when you pause.",
  "已插入编辑器": "Inserted into editor",
  "PDF 选项": "PDF options",
  "页码范围（如 1-3,5，留空为全部）": "Pages (e.g. 1-3,5; empty for all)",
  "共 {count} 页": "{count} pages",
  "开始识别": "Recognize",
  "正在识别第 {page}/{total} 页": "Recognizing page {page} of {total}",
  "PDF 识别结果": "PDF result",
  "已停止": "Stopped",
  "停止": "Stop",
  "页码范围无效": "Invalid page range",
  "无法打开 PDF：{error}": "Can’t open the PDF: {error}",
  "清晰文档 150–200，扫描件 300": "150–200 for clean documents, 300 for scans",
  "下一步": "Next",
  "上一步": "Back",
  "开始使用": "Get started",
  "图片、PDF 与手写": "Images, PDFs and handwriting",
  "可以拖入图片、粘贴截图、识别整份 PDF，或直接手写公式。":
    "Drop images, paste a screenshot, recognize a whole PDF, or write a formula by hand.",
  "无法渲染：{error}": "Can’t render: {error}",
  "拖到这里识别": "Drop to recognize",
  "跳过": "Skip",
  "按 {hotkey} 或点这里，框选屏幕上的公式。第一次使用时，macOS 会请求屏幕录制权限。":
    "Press {hotkey} or click here, then drag over a formula on screen. The first time, macOS asks for Screen Recording permission.",
  "也可以识别图片文件、整份 PDF，或直接手写公式。把图片拖进窗口，或在窗口里按 {paste} 粘贴也行。":
    "You can also recognize image files, whole PDFs or handwriting. Dropping an image on the window or pasting with {paste} works too.",
  "收藏夹与设置": "Favorites and settings",
  "在这里打开收藏夹、清空历史。识别类型、模型下载、主题和语言都在“设置”里。":
    "Open your favorites or clear the history here. Recognition type, model downloads, theme and language are in Settings.",
  "关闭窗口后应用仍在后台运行，可以从系统托盘图标重新打开或退出。想再看一遍引导，点这个按钮。":
    "Closing the window keeps the app running: reopen or quit it from the system tray icon. Click this button to see this tour again.",
  "编辑与预览": "Edit and preview",
  "识别结果会先在确认窗口里让你检查和修改，确认后填入这里。下方的方格纸实时渲染公式。":
    "Each result opens for review first; once you confirm, it lands here. The graph paper below renders it as you type.",
  "复制与导出": "Copy and export",
  "复制编辑器里的内容，或导出为 TeX、Markdown、SVG 等格式。导出菜单里还能复制为 $…$、equation 或 MathML。":
    "Copy what’s in the editor, or export it as TeX, Markdown, SVG and more. The export menu can also copy it as $…$, equation or MathML.",
  "每次确认的结果都记在这里。点一条载入编辑器，右键可以重命名、收藏或删除。":
    "Every confirmed result is kept here. Click one to load it into the editor; right-click to rename, favorite or delete it.",
  "快捷键与设置": "Shortcut and settings",
  "在这里更改截图快捷键、打开收藏夹。识别类型、模型下载、主题和语言都在“设置”里。":
    "Change the capture shortcut and open your favorites here. Recognition type, model downloads, theme and language are in Settings.",
  "后台运行": "Runs in the background",
  "关闭窗口后应用仍在后台运行，用 {hotkey} 或菜单栏的 Σ 图标随时截图，按 ⌘Q 完全退出。想再看一遍引导，点这个按钮。":
    "Closing the window keeps the app running: capture any time with {hotkey} or the Σ menu bar icon, and press ⌘Q to quit. Click this button to see this tour again.",
  "MathCraft 模型": "MathCraft models",
  "已下载 {n}/{total}": "{n} of {total} downloaded",
  "公式检测": "Formula detection",
  "公式识别": "Formula recognition",
  "文字检测": "Text detection",
  "文字识别": "Text recognition",
  "用于：{modes}": "Used by: {modes}",
  "已下载": "Downloaded",
  "未下载": "Not downloaded",
  "下载中…": "Downloading…",
  "下载中 {percent}%": "Downloading {percent}%",
  "下载失败": "Download failed",
  "下载": "Download",
  "重新下载": "Download again",
  "{name} 已下载": "{name} downloaded",
  "该模型正在下载": "This model is already downloading",
  "删除“{name}”模型？{modes}识别将无法使用，需要时可重新下载（{size}）。":
    "Delete the {name} model? {modes} recognition won’t work until you download it again ({size}).",
  "{name} 已删除": "{name} deleted",
  "删除失败：{error}": "Delete failed: {error}",
};

let current: Lang = "zh";

export function setLang(lang: Lang): void {
  current = lang;
}

export function getLang(): Lang {
  return current;
}

export function t(key: string, vars: Record<string, string | number> = {}): string {
  const template = current === "en" ? (EN[key] ?? key) : key;
  return template.replace(/\{(\w+)\}/g, (m, name) => (name in vars ? String(vars[name]) : m));
}

/** Apply translations to static markup tagged with data-i18n / data-i18n-placeholder. */
export function translateDom(root: ParentNode): void {
  root.querySelectorAll<HTMLElement>("[data-i18n]").forEach((el) => {
    el.textContent = t(el.dataset.i18n!);
  });
  root.querySelectorAll<HTMLElement>("[data-i18n-placeholder]").forEach((el) => {
    el.setAttribute("placeholder", t(el.dataset.i18nPlaceholder!));
  });
  root.querySelectorAll<HTMLElement>("[data-i18n-title]").forEach((el) => {
    el.setAttribute("title", t(el.dataset.i18nTitle!));
  });
}

export const translationKeys = Object.keys(EN);
