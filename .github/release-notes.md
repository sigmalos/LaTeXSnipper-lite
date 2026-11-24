> [!IMPORTANT]
> **当前版本的 Windows 和 Linux 版无法进行截图识别**，「截图识别」按钮和全局快捷键只在 macOS 上提供。Windows 和 Linux 版可以识别图片文件、拖入或粘贴的截图、PDF 和手写。

## {{VERSION}} 更新内容

### 新功能

- **每个识别部分都可以选模型**：「设置 → 识别后端」改为「本地模型」和「在线模型」两项。本地模型下，公式检测、公式识别、文字检测、文字识别各有一个下拉列表，选中一个模型后可以在同一行下载、重新下载或删除它，点「保存」后识别就改用它，点「取消」不会改变。状态栏显示正在使用的模型，比如「当前模型: Texo 公式」。
- **公式识别可选用 Texo**：[Texo](https://github.com/alephpi/Texo) 是蒸馏自 PP-FormulaNet-S 的轻量公式识别模型，这里使用它的 Q8_0 量化 GGUF 版本（22 MB，来自 [cstr/texo-distill-gguf](https://huggingface.co/cstr/texo-distill-gguf)，经 hf-mirror 下载并校验 SHA-256）。它不会自动下载，在公式识别的下拉列表中选中后点「下载」即可；选中但还没下载时开始识别，会提示下载。Texo 的输出会多一层花括号分组，用 `\infin` 表示无穷，渲染结果不受影响。Texo 的权重以 AGPL-3.0 协议发布。
- **支持 GGUF 模型**：应用可以直接读取 GGUF 格式的公式识别模型，把权重组装成计算图交给 ONNX Runtime 运行，和其他模型一样优先使用 GPU，出错时自动改用 CPU。

### 其他

- 默认使用的四个 MathCraft 模型和第一次打开时的自动下载都没有变化。
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

- **从 1.4.1 升级**：应用会自动更新到这个版本，也可以在「设置」底部点「检查更新」。设置、历史、收藏和已下载的模型都会保留，默认仍使用原来的 MathCraft 模型。
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
