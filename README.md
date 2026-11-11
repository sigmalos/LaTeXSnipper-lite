# LaTeXSnipper（macOS · Tauri）

把屏幕截图、图片、PDF 和手写转成可编辑的 LaTeX。这是原 PyQt 版 LaTeXSnipper 的 macOS 专版重写：Rust + Tauri 2 + 原生 WKWebView，不再依赖 Python、Qt 或 Chromium。

| | 原版（PyQt6 + WebEngine） | 本版（Tauri） |
|---|---|---|
| 安装包 | 内置 Python、Qt、Chromium、模型 | DMG 12 MB（.app 23 MB） |
| OCR | Python + onnxruntime + OpenCV + RapidOCR | Rust 原生移植，ONNX Runtime 静态链接 |
| 平台 | Windows / Linux / macOS | 仅 macOS 13+（Apple Silicon） |

## 功能

- **截图识别**：调用系统 `screencapture` 框选区域，全局快捷键默认 `⌘F`（可在“快捷键”中修改）
- **后台运行与菜单栏**：关闭窗口后应用继续在后台运行，菜单栏图标提供截图识别、打开主窗口、设置与退出；点击 Dock 图标重新打开窗口，`⌘Q` 完全退出
- **图片识别**：选择文件、拖入窗口，或在窗口内 `⌘V` 粘贴截图
- **PDF 识别**：pdf.js 逐页渲染（可选页码和 DPI），按混合模式识别并合并成 Markdown
- **手写识别**：画布书写，停笔后自动识别，支持橡皮和撤销
- **三种识别类型**：公式 / 混合（文字 + `$…$`）/ 纯文字，与原版 MathCraft 输出逐字一致
- **外部模型**：可选任何 OpenAI 兼容视觉接口（OpenAI、Ollama `/v1`、LM Studio、vLLM 等）
- **编辑与预览**：MathJax 3 实时渲染；导出 TeX / Markdown / HTML / MathML / SVG / TXT；装有 Pandoc 时可导出 Word / ODT
- **历史与收藏**：重命名、收藏、删除、排序；数据目录与原版共用（`~/Library/Application Support/LaTeXSnipper`），原有 `history.json`、`favorites.json` 和已下载的模型直接沿用

## 模型

模型共四个，不打进 DMG，从 [MathCraft-Models](https://github.com/SakuraMathcraft/MathCraft-Models/releases) 下载并逐文件做 SHA-256 校验：

| 模型 | 用于 | 下载大小 |
|---|---|---|
| 公式检测 | 混合 | 64 MB |
| 公式识别 | 公式、混合 | 104 MB |
| 文字检测 | 混合、纯文字 | 4.3 MB |
| 文字识别 | 混合、纯文字 | 13 MB |

首次使用某种识别类型时会提示下载所需模型；也可以在“设置 → 识别后端 → 内置 MathCraft”中查看每个模型的状态，单独下载、重新下载或删除（删除前会确认）。下载或删除后立即生效，无需重启。已经装过原版的机器无需下载。

## 开发

```sh
npm install
npm run tauri dev           # 开发模式
npm run dmg                 # 生成 backend/target/release/bundle/dmg/*.dmg
```

需要 Rust（版本由 `rust-toolchain.toml` 固定）和 Node 20+。

## 测试

```sh
npm run test:rust           # Rust 单元测试 + 真实模型对拍（无模型时自动跳过；LATEXSNIPPER_MODELS 可指定模型目录）
npm test                    # 前端单元测试（Vitest）
npm run test:e2e            # 界面 E2E（Playwright · 无头 WebKit，IPC 由 e2e/tauri-mock.js 模拟）
./scripts/smoke-dmg.sh      # 安装包 E2E：挂载 DMG、检查链接、无界面识别、无头启动自检
```

- `backend/tests/ocr_parity.rs` 用真实模型跑 `tests/fixtures/*.png`，结果必须与原 Python MathCraft 的输出（`python_reference.json`）完全一致
- Tauri 的 WebDriver 不支持 macOS，所以界面 E2E 在同引擎的 WebKit 上运行，后端由模拟层替代；真实后端由 Rust 集成测试和 `smoke-dmg.sh` 覆盖
- 所有测试都不打扰本机：E2E 使用无头 WebKit（不弹窗口、不抢焦点）；`smoke-dmg.sh` 以 `LATEXSNIPPER_HEADLESS=1` 启动应用，不显示窗口、Dock 图标和菜单栏图标，也不注册全局快捷键，由前端渲染一个公式并通过 IPC 回报后自动退出

## 结构

```
frontend/               前端（TypeScript，无框架）
  main.ts               界面逻辑
  lib/                  format / i18n / math(MathJax) / ink / pdf / api
backend/src/            后端（Rust + Tauri）
  ocr/                  MathCraft OCR 的 Rust 移植：模型、分词器、版面、LaTeX 后处理
  commands.rs           IPC 命令
  shell.rs              菜单栏图标、后台运行、全局快捷键
  store.rs              设置、历史、收藏（兼容原版格式）
  modelhub.rs           模型下载与校验
  external.rs           OpenAI 兼容接口
```

## 相比原版移除的部分

Windows / Linux 平台代码与安装器、Office 插件（仅支持 Windows）、自动化 HTTP API、应用内更新器、MinerU 客户端，以及 Python 运行时与打包脚本。

## 许可

GPL-3.0-only，见 `LICENSE`。
