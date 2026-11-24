> [!IMPORTANT]
> **当前版本的 Windows 和 Linux 版无法进行截图识别**，「截图识别」按钮和全局快捷键只在 macOS 上提供。Windows 和 Linux 版可以识别图片文件、拖入或粘贴的截图、PDF 和手写。

## {{VERSION}} 更新内容

### 改进与修复

- **只内置四个模型，全部默认使用 Int8 版本**：「设置 → 识别后端 → 内置 MathCraft」中只列出公式检测、公式识别、文字检测、文字识别四个模型，公式检测和公式识别固定使用体积更小、在 CPU 上更快的 Int8 版本，不再需要在标准版和 Int8 版之间选择，「选用」按钮已去掉。
- **第一次打开时自动下载模型**：第一次打开应用时，会在后台依次下载四个模型（共约 62 MB），先下载公式识别，状态栏显示「正在下载模型…」。下载中断后，下次打开会继续；四个都下载完以后不再自动下载，手动删除的模型不会被重新下载。下载完成前就开始识别时，会等待正在下载的模型，而不是报错。
- **发布文件更整齐**：Linux 安装包改名为 `LaTeXSnipper-Lite_{{VERSION}}_linux-amd64.deb`，与 macOS、Windows 的命名一致；Office 插件改用全小写的 `latexsnipper-office_…` 以便和桌面端区分。每个版本只发布四个安装包和自动更新需要的两个文件。
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

- **从 1.4.0 升级**：应用会自动更新到这个版本，也可以在「设置」底部点「检查更新」。更新后，之前下载的标准版公式检测、公式识别模型不再使用，应用会在后台自动下载 Int8 版本（约 44 MB）。不再使用的 `mathcraft-formula-det`、`mathcraft-formula-rec` 文件夹可以手动删除；macOS 上原版 LaTeXSnipper 和 Office 插件仍使用 `mathcraft-formula-rec`，与原版共用数据时请保留它。
- **从 1.3.x 升级**：1.3.2 及更早的版本没有自动更新，需要手动安装一次这个版本，模型、历史、收藏和设置都会保留。macOS 上这一次需要重新允许屏幕录制，以后的更新会保留这个权限。
- **从 1.2.0 或 1.1.0 升级**：所有系统直接安装新版本即可，历史、收藏和设置都会保留，公式模型改用 Int8 版本并自动下载。
- **从 1.0.0 升级（macOS / Windows）**：直接安装新版本即可，模型、历史和收藏都会保留。
- **从 1.0.0 升级（Linux）**：1.0.0 的 deb 包名是 `la-te-x-snipper`，新版改为 `la-te-x-snipper-lite`，apt 不会把它当作升级。请先卸载旧包、把数据移到新目录，**再第一次启动新版本**：

  ```sh
  sudo apt remove la-te-x-snipper
  mkdir -p "${XDG_DATA_HOME:-$HOME/.local/share}"
  mv "$HOME/Library/Application Support/LaTeXSnipper" "${XDG_DATA_HOME:-$HOME/.local/share}/latexsnipper-lite"
  sudo apt install ./LaTeXSnipper-Lite_{{VERSION}}_linux-amd64.deb
  ```

  不移动旧数据也能使用，只是需要重新下载模型，历史和收藏从空白开始。
