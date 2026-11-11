## 下载

| 系统 | 文件 |
|---|---|
| macOS 13+，Apple Silicon | `LaTeXSnipper-Lite_{{VERSION}}_macos-arm64.dmg` |
| Windows 10/11，x64 | `LaTeXSnipper-Lite_{{VERSION}}_windows-x64-setup.exe` |
| Linux x64（Debian / Ubuntu） | `latexsnipper-lite_{{VERSION}}_amd64.deb` |
| Linux x64（其他发行版） | `LaTeXSnipper-Lite_{{VERSION}}_linux-x86_64.AppImage` |

- **macOS**：安装包未经 Apple 公证，首次打开若提示无法验证，请在「系统设置 → 隐私与安全性」底部点「仍要打开」。
- **Windows**：安装包未签名，SmartScreen 提示时点「更多信息 → 仍要运行」。
- **截图识别**目前仅支持 macOS；Windows 和 Linux 版可以识别图片文件、拖入或粘贴的截图、PDF 和手写。
- 识别模型在第一次识别时按需下载，并逐文件校验 SHA-256。

校验文件完整性：`sha256sum -c SHA256SUMS.txt`（macOS 用 `shasum -a 256 -c SHA256SUMS.txt`）。
