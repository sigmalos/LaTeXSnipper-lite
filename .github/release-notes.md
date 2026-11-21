> [!IMPORTANT]
> **当前版本的 Windows 和 Linux 版无法进行截图识别**，「截图识别」按钮和全局快捷键只在 macOS 上提供。Windows 和 Linux 版可以识别图片文件、拖入或粘贴的截图、PDF 和手写。

## {{VERSION}} 更新内容

### 新功能

- **Word / PowerPoint for Mac 插件重写，不再依赖桌面端**：插件的宿主程序从 Python 改为 Rust，打包成单独的「LaTeXSnipper Office」应用。截图识别直接在插件里完成，使用内置的 MathCraft 引擎，无需另外运行桌面端。插件页面、接口和 Word 中公式的保存格式与原版一致，原版插件插入的公式可以继续编辑。已在 Word 和 PowerPoint 16.115 中验证插入、读取、修改、删除、编号、PNG / SVG 转换和截图识别。
- **GPU 加速（Windows / Linux）**：Windows 版通过 DirectML 使用显卡（NVIDIA / AMD / Intel，自动选显存最大的一块），所需的 `DirectML.dll` 已随安装包附带。Linux 版在装好 CUDA 12、cuDNN 9，并放入 ONNX Runtime 1.22 的 CUDA 模块后使用 NVIDIA 显卡，步骤见 README。GPU 无法使用或在 GPU 上运行出错时自动改用 CPU；设置环境变量 `LATEXSNIPPER_DEVICE=cpu` 可以强制使用 CPU。macOS 版不变。
- **快速入门改为界面引导**：原来的四页说明换成七步引导，依次高亮界面上真实的按钮和区域并说明用法。首次启动时出现一次，之后可以点左上角的「?」重看。

### 改进与修复

- **后台截图只弹出识别结果窗口（macOS）**：主窗口已关闭或隐藏时，用快捷键（默认 ⌘F）或菜单栏截图，识别成功后之前会把主窗口一起弹出来。现在只弹出一个小的识别结果窗口，点「确认并复制」后结果复制到剪贴板、记入历史，窗口收起，焦点回到截图前正在使用的应用，可以直接粘贴。识别出错或需要下载模型时仍会打开主窗口。
- **Linux 数据目录改为遵循 XDG 规范**：1.0.0 在 Linux 上误用了 macOS 的路径 `~/Library/Application Support/LaTeXSnipper`，现在改为 `~/.local/share/latexsnipper-lite`（设置了 `XDG_DATA_HOME` 时放在其下）。从 1.0.0 升级请看下方「从 1.0.0 升级」。
- **导出时能找到 PATH 中的 Pandoc**：之前只在 Homebrew 的安装目录里查找，Windows、Linux 以及其他方式安装的 Pandoc 都找不到，依赖 Pandoc 的导出格式无法使用。
- **Windows / Linux 托盘图标改为彩色**：之前用的是 macOS 菜单栏的黑色模板图标，在深色任务栏上看不见。
- **Windows / Linux 界面细节**：快速入门跳过截图识别这一步和只适用于 macOS 的说明；左下角工具栏不再在隐藏的「快捷键」按钮处留下空位。
- **不再提供 AppImage**：Linux 版只提供 `.deb` 安装包。
- 安装包文件名统一为 `LaTeXSnipper-Lite_<版本>_<系统>`，每个版本附带 `SHA256SUMS.txt`。所有安装包在发布前都会在对应系统上安装并用真实模型做识别测试。

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
- 识别模型不在安装包里，第一次识别时按需下载（只用公式识别约 104 MB），并逐文件校验 SHA-256。

## 从 1.0.0 升级

- **macOS / Windows**：直接安装新版本即可，模型、历史和收藏都会保留。
- **Linux**：1.0.0 的 deb 包名是 `la-te-x-snipper`，新版改为 `la-te-x-snipper-lite`，apt 不会把它当作升级。请先卸载旧包、把数据移到新目录，**再第一次启动新版本**：

  ```sh
  sudo apt remove la-te-x-snipper
  mkdir -p "${XDG_DATA_HOME:-$HOME/.local/share}"
  mv "$HOME/Library/Application Support/LaTeXSnipper" "${XDG_DATA_HOME:-$HOME/.local/share}/latexsnipper-lite"
  sudo apt install ./latexsnipper-lite_{{VERSION}}_amd64.deb
  ```

  不移动旧数据也能使用，只是需要重新下载模型，历史和收藏从空白开始。

校验文件完整性：`sha256sum -c SHA256SUMS.txt`（macOS 用 `shasum -a 256 -c SHA256SUMS.txt`）。
