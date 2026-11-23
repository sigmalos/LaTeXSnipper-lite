> [!IMPORTANT]
> **当前版本的 Windows 和 Linux 版无法进行截图识别**，「截图识别」按钮和全局快捷键只在 macOS 上提供。Windows 和 Linux 版可以识别图片文件、拖入或粘贴的截图、PDF 和手写。

## {{VERSION}} 更新内容

### 改进与修复

- **截图识别缺少屏幕录制权限时给出提示**：macOS 没有允许 LaTeXSnipper 录制屏幕时，按 ⌘F 或点「截图识别」以前没有任何反应，现在会打开主窗口说明原因，并提供「打开系统设置」按钮，直接进入「隐私与安全性 → 屏幕录制」。第一次缺少权限时，应用会自动出现在屏幕录制列表中。
- **实时渲染预览中的公式改为固定大小**：单个公式以固定的字号显示在方格纸正中，不再随窗口放大或缩小；比预览区域更宽的公式可以左右滚动。
- Word / PowerPoint for Mac 插件没有功能变化，版本仍为 {{OFFICE_VERSION}}，继续使用标准版公式识别模型。

1.3.0 的新功能（主界面的「识别类型」下拉列表、Int8 公式检测模型和「选用」按钮）见 [1.3.0 的发布说明](https://github.com/sigmalos/LaTeXSnipper-lite/releases/tag/v1.3.0)。

## 下载

| 系统 | 文件 |
|---|---|
| macOS 13+，Apple Silicon | `LaTeXSnipper-Lite_{{VERSION}}_macos-arm64.dmg` |
| Windows 10 / 11，x64 | `LaTeXSnipper-Lite_{{VERSION}}_windows-x64-setup.exe` |
| Linux x64（Debian / Ubuntu 等） | `latexsnipper-lite_{{VERSION}}_amd64.deb` |
| Word / PowerPoint for Mac 插件，Apple Silicon | `LaTeXSnipper-Office_{{OFFICE_VERSION}}_macos-arm64.dmg` |
| 源代码 | 页面底部的 `Source code (zip)` / `Source code (tar.gz)` |

- **macOS**：安装包未经 Apple 公证，首次打开若提示无法验证，请在「系统设置 → 隐私与安全性」底部点「仍要打开」。第一次截图时按提示在「屏幕录制」中允许 LaTeXSnipper。
- **Windows**：安装包未签名，SmartScreen 提示时点「更多信息 → 仍要运行」。安装无需管理员权限；缺少 Microsoft Edge WebView2 Runtime 时安装程序会自动下载。应用以「LaTeXSnipper Lite」的名字单独安装，可与原版同时使用，数据保存在 `%LOCALAPPDATA%\LaTeXSnipper Lite`。
- **Windows 上代替截图识别**：先用系统自带的 **Win + Shift + S** 截图，再回到应用按 **Ctrl + V** 粘贴识别。
- **Linux**：`sudo apt install ./latexsnipper-lite_{{VERSION}}_amd64.deb`，依赖的 WebKitGTK 等库由 apt 自动安装。
- **Office 插件**：单独安装，不依赖桌面端。打开 DMG 把「LaTeXSnipper Office」拖进「应用程序」，打开后点「安装」，步骤见 `office_plugin/README.md`。
- 识别模型不在安装包里，第一次识别时按需下载（只用公式识别时，标准版约 104 MB，Int8 版约 24 MB），并逐文件校验 SHA-256。

## 升级

- **从 1.3.1 或 1.3.0 升级**：所有系统直接安装新版本即可，模型、历史、收藏和设置都会保留。
- **macOS 升级后需要重新允许屏幕录制**：安装包未经 Apple 签名，每个新版本在 macOS 看来都是新的应用，之前授予的屏幕录制权限不再生效。升级后第一次截图时按提示操作即可；如果「屏幕录制」列表中的 LaTeXSnipper 已经打开但仍无法截图，用「−」把它移除，重新打开应用后再截图一次，在弹出的提示中允许。
- **从 1.2.0 或 1.1.0 升级**：所有系统直接安装新版本即可，模型、历史、收藏和设置都会保留，识别类型和已选的公式识别版本保持不变，公式检测默认使用标准版。
- **从 1.0.0 升级（macOS / Windows）**：直接安装新版本即可，模型、历史和收藏都会保留。
- **从 1.0.0 升级（Linux）**：1.0.0 的 deb 包名是 `la-te-x-snipper`，新版改为 `la-te-x-snipper-lite`，apt 不会把它当作升级。请先卸载旧包、把数据移到新目录，**再第一次启动新版本**：

  ```sh
  sudo apt remove la-te-x-snipper
  mkdir -p "${XDG_DATA_HOME:-$HOME/.local/share}"
  mv "$HOME/Library/Application Support/LaTeXSnipper" "${XDG_DATA_HOME:-$HOME/.local/share}/latexsnipper-lite"
  sudo apt install ./latexsnipper-lite_{{VERSION}}_amd64.deb
  ```

  不移动旧数据也能使用，只是需要重新下载模型，历史和收藏从空白开始。

校验文件完整性：`sha256sum -c SHA256SUMS.txt`（macOS 用 `shasum -a 256 -c SHA256SUMS.txt`）。
