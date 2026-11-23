> [!IMPORTANT]
> **当前版本的 Windows 和 Linux 版无法进行截图识别**，「截图识别」按钮和全局快捷键只在 macOS 上提供。Windows 和 Linux 版可以识别图片文件、拖入或粘贴的截图、PDF 和手写。

## {{VERSION}} 更新内容

### 新功能

- **识别类型移到主界面**：「截图识别」按钮下方新增「识别类型」下拉列表，可在公式、混合、纯文字之间切换，选中立即生效，不用再打开设置保存。下拉列表沿用应用自己的菜单样式，深色主题下同样适配，也可以用方向键、回车和 Esc 操作。设置中不再有「识别类型」一栏，快速入门引导相应增加一步。截图、图片和粘贴识别按所选类型进行；PDF 仍按混合识别，手写仍按公式识别。
- **可选 Int8 公式检测模型**：混合识别和 PDF 识别使用的公式检测模型新增 Int8 版本，下载约 20 MB（标准版约 64 MB），从 Gitee 下载，国内网络无需代理。在测试图片上，混合识别耗时约减少四分之一，识别结果与标准版相同；个别公式的检测框可能不如标准版准确。默认仍使用标准版。
- **用「选用」按钮选择模型版本**：「设置 → 识别后端 → 内置 MathCraft」的模型列表中，公式检测和公式识别的标准版与 Int8 版右侧各有一个「选用」按钮。每一对只能选一个，两对互不影响，点「保存」后生效。原来的「公式识别模型」切换按钮已去掉。鼠标移到 Int8 版旁的问号上，可以看到它与标准版（FP32）相比的优缺点。

### 改进与修复

- 公式检测的两个版本只要下载了其中一个，混合识别和 PDF 识别就能使用：所选版本未下载而另一个已下载时，直接用已下载的那个，不再提示下载。
- 「下载模型」提示中的大小按实际需要下载的模型计算。
- 模型列表中各行的按钮、大小和状态上下对齐。
- Word / PowerPoint for Mac 插件没有功能变化，版本仍为 {{OFFICE_VERSION}}，继续使用标准版公式识别模型。

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
