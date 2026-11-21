> [!IMPORTANT]
> **当前版本的 Windows 和 Linux 版无法进行截图识别**，「截图识别」按钮和全局快捷键只在 macOS 上提供。Windows 和 Linux 版可以识别图片文件、拖入或粘贴的截图、PDF 和手写。

## 下载

| 系统 | 文件 |
|---|---|
| macOS 13+，Apple Silicon | `LaTeXSnipper-Lite_{{VERSION}}_macos-arm64.dmg` |
| Windows 10/11，x64 | `LaTeXSnipper-Lite_{{VERSION}}_windows-x64-setup.exe` |
| Linux x64（Debian / Ubuntu） | `latexsnipper-lite_{{VERSION}}_amd64.deb` |
| Linux x64（其他发行版） | `LaTeXSnipper-Lite_{{VERSION}}_linux-x86_64.AppImage` |
| Word / PowerPoint for Mac 插件 | `LaTeXSnipper-Office_<版本>_macos-arm64.dmg` |

- **macOS**：安装包未经 Apple 公证，首次打开若提示无法验证，请在「系统设置 → 隐私与安全性」底部点「仍要打开」。
- **Windows**：安装包未签名，SmartScreen 提示时点「更多信息 → 仍要运行」。安装无需管理员权限；缺少 Microsoft Edge WebView2 Runtime 时安装程序会自动下载。应用以「LaTeXSnipper Lite」的名字单独安装，可与原版同时使用，数据保存在 `%LOCALAPPDATA%\LaTeXSnipper Lite`。
- **Windows 上代替截图识别**：先用系统自带的 **Win + Shift + S** 截图，再回到应用按 **Ctrl + V** 粘贴识别。
- **GPU 加速**：Windows 版通过 DirectML 自动使用显卡（NVIDIA / AMD / Intel）。Linux 版在装好 CUDA 12、cuDNN 9，并放入 ONNX Runtime 1.22 的 CUDA 模块后使用 NVIDIA 显卡，步骤见 README。没有可用的 GPU 时自动改用 CPU。
- 识别模型在第一次识别时按需下载，并逐文件校验 SHA-256。
- **Office 插件**：单独安装，不依赖桌面端，截图识别也在插件内完成。打开 DMG 把「LaTeXSnipper Office」拖进「应用程序」，打开后点「安装」，步骤见 `office_plugin/README.md`。

校验文件完整性：`sha256sum -c SHA256SUMS.txt`（macOS 用 `shasum -a 256 -c SHA256SUMS.txt`）。
