> [!IMPORTANT]
> **当前版本的 Windows 和 Linux 版无法进行截图识别**，「截图识别」按钮和全局快捷键只在 macOS 上提供。Windows 和 Linux 版可以识别图片文件、拖入或粘贴的截图、PDF 和手写。

## {{VERSION}} 更新内容

### 新功能

- **自动更新**：联网时，应用会在启动后和之后每隔几小时检查 GitHub 上的最新版本，发现新版本就在后台下载并校验签名。macOS 上下载完成后直接替换应用，下次打开即是新版本；Windows 上在退出 LaTeXSnipper 时静默安装；Linux 上点「设置」里的「重启以完成更新」安装（需要输入系统密码）。
- **检查更新**：「设置」底部新增「检查更新」按钮。有新版本时按钮变为「更新到 x.y.z」，点它会下载、安装并重启；已是最新版本时会给出提示。
- **更新日志**：更新后第一次打开应用，会显示这一版的更新内容，只显示一次。

### 改进与修复

- **macOS 版改用固定的证书签名**：以前每个版本在 macOS 看来都是新的应用，更新后屏幕录制权限会失效，按 ⌘F 截图没有反应。从这一版起，应用用同一个证书签名，以后更新时屏幕录制权限会保留。
- Word / PowerPoint for Mac 插件没有功能变化，版本仍为 {{OFFICE_VERSION}}，继续使用标准版公式识别模型。

## 下载

| 系统 | 文件 |
|---|---|
| macOS 13+，Apple Silicon | `LaTeXSnipper-Lite_{{VERSION}}_macos-arm64.dmg` |
| Windows 10 / 11，x64 | `LaTeXSnipper-Lite_{{VERSION}}_windows-x64-setup.exe` |
| Linux x64（Debian / Ubuntu 等） | `LaTeXSnipper-Lite_{{VERSION}}_linux-amd64.deb` |
| Word / PowerPoint for Mac 插件，Apple Silicon | `latexsnipper-office_{{OFFICE_VERSION}}_macos-arm64.dmg` |
| 源代码 | 页面底部的 `Source code (zip)` / `Source code (tar.gz)` |

- **macOS**：安装包未经 Apple 公证，首次打开若提示无法验证，请在「系统设置 → 隐私与安全性」底部点「仍要打开」。第一次截图时按提示在「屏幕录制」中允许 LaTeXSnipper。
- **Windows**：安装包未签名，SmartScreen 提示时点「更多信息 → 仍要运行」。安装无需管理员权限；缺少 Microsoft Edge WebView2 Runtime 时安装程序会自动下载。应用以「LaTeXSnipper Lite」的名字单独安装，可与原版同时使用，数据保存在 `%LOCALAPPDATA%\LaTeXSnipper Lite`。
- **Windows 上代替截图识别**：先用系统自带的 **Win + Shift + S** 截图，再回到应用按 **Ctrl + V** 粘贴识别。
- **Linux**：`sudo apt install ./LaTeXSnipper-Lite_{{VERSION}}_linux-amd64.deb`，依赖的 WebKitGTK 等库由 apt 自动安装。
- **Office 插件**：单独安装，不依赖桌面端。打开 DMG 把「LaTeXSnipper Office」拖进「应用程序」，打开后点「安装」，步骤见 `office_plugin/README.md`。
- 识别模型不在安装包里，第一次打开应用时自动在后台下载四个模型（共约 62 MB），并逐文件校验 SHA-256。

## 升级

- **从 1.3.x 升级**：1.3.2 及更早的版本没有自动更新，需要手动安装一次这个版本，模型、历史、收藏和设置都会保留。之后的新版本会自动更新。
- **macOS 最后一次重新允许屏幕录制**：这一版的签名方式变了，升级后第一次截图时按提示重新允许屏幕录制；如果「屏幕录制」列表中的 LaTeXSnipper 已经打开但仍无法截图，用「−」把它移除，重新打开应用后再截图一次，在弹出的提示中允许。以后的更新会保留这个权限。
- **从 1.2.0 或 1.1.0 升级**：所有系统直接安装新版本即可，模型、历史、收藏和设置都会保留，识别类型和已选的公式识别版本保持不变，公式检测默认使用标准版。
- **从 1.0.0 升级（macOS / Windows）**：直接安装新版本即可，模型、历史和收藏都会保留。
- **从 1.0.0 升级（Linux）**：1.0.0 的 deb 包名是 `la-te-x-snipper`，新版改为 `la-te-x-snipper-lite`，apt 不会把它当作升级。请先卸载旧包、把数据移到新目录，**再第一次启动新版本**：

  ```sh
  sudo apt remove la-te-x-snipper
  mkdir -p "${XDG_DATA_HOME:-$HOME/.local/share}"
  mv "$HOME/Library/Application Support/LaTeXSnipper" "${XDG_DATA_HOME:-$HOME/.local/share}/latexsnipper-lite"
  sudo apt install ./LaTeXSnipper-Lite_{{VERSION}}_linux-amd64.deb
  ```

  不移动旧数据也能使用，只是需要重新下载模型，历史和收藏从空白开始。
