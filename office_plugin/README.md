# LaTeXSnipper Office for Mac

在 Word 和 PowerPoint for Mac 中插入、编辑可以再次修改的 LaTeX 公式，并在插件里直接截图识别。

插件是一个单独的应用 **LaTeXSnipper Office.app**，有自己的 DMG，与桌面端分开构建、分开安装。装好后它作为登录项在后台运行，**不需要打开，也不需要安装 LaTeXSnipper 桌面端**：编辑、渲染、插入、设置、帮助和截图识别都由插件自己完成。它与桌面端唯一的交集是 MathCraft 模型：桌面端已经下载过公式识别模型时，插件直接读取（只读），否则由插件自己下载一份。

## 安装

**系统要求**：macOS 13 或更新版本，Apple Silicon；Microsoft 365、Office 2021 或更新版本的 Word / PowerPoint for Mac。

1. 打开 `LaTeXSnipper-Office_<版本>_macos-arm64.dmg`，把 **LaTeXSnipper Office** 拖进「应用程序」，再从「应用程序」打开它（直接在 DMG 里打开会提示先拖进「应用程序」）。安装包没有经过 Apple 公证，首次打开的处理方法和桌面端相同。
2. 先打开一次 Word 和（或）PowerPoint，然后回到 LaTeXSnipper Office 点「安装」。macOS 会要求输入密码，用于信任一张只对 `localhost` 有效的本地证书。Office 只从 HTTPS 加载插件页面，所以需要这张证书。
3. 需要截图识别时，在「截图识别模型」一行点「下载」（约 109 MB，逐文件校验 SHA-256）。已经用 LaTeXSnipper 下载过模型的话，这一行会显示「使用 LaTeXSnipper 已下载的模型」，不用再下载。
4. 重新打开 Word 或 PowerPoint，在「开始 › 加载项」中选择 LaTeXSnipper。

使用中 macOS 会再询问两次授权，都需要允许：

- **控制 Microsoft Word**：第一次在 Word 中保存公式时。Word 公式的元数据存在文档变量里，Office.js 只能读不能写，只有 Word 的 AppleScript 接口可以写入。
- **屏幕录制**：第一次截图识别时。授权后如果仍然只能截到桌面背景，到「系统设置 › 隐私与安全性 › 屏幕录制」中确认 LaTeXSnipper Office 已打开。

在同一个窗口里点两次「卸载」，会停止并移除登录项、两个 Office 中的插件清单、证书和它的信任设置、插件设置，以及插件自己下载的模型。桌面端的数据和模型不受影响。

### 与原版 LaTeXSnipper Office 的关系

本插件沿用原版 Mac 插件的应用名、加载项 ID、登录项、端口（28766）和设置文件，安装后会原地替换原版插件。原版留下的证书（`~/Library/Application Support/LaTeXSnipper Office/localhost.crt`）在有效期内会继续使用，`settings.json` 也按原格式读取。

## 功能

功能区、任务窗格、编辑器、设置和帮助页都直接沿用原版的 Mac 插件，按原版 Windows 插件的功能区分组。

| 宿主 | 分组与命令 |
| --- | --- |
| Word | **公式**：行内、行间、带编号公式、截图识别 · **编辑**：加载所选、删除所选 · **公式解析**：解析所选、解析全文 · **交叉引用**：插入引用 · **编号**：添加编号、全部重编号 · **编号分隔**：章分隔符、节分隔符 · **公式样式**：格式化所选、格式化全部 · **工具**：状态窗格、设置、帮助 |
| PowerPoint | **公式**：插入公式、截图识别 · **编辑**：加载所选、删除所选 · **转换**：转为 PNG、转为 SVG · **公式样式**：格式化所选、格式化全部 · **工具**：状态窗格、设置、帮助 |

- Word 公式都是原生 OMML 公式，用 Word 自带的 `mathml2omml.xsl` 转换，元数据格式与原版 Windows 插件相同，两个平台插入的公式可以互相加载、编辑和重编号。
- PowerPoint 公式是 PNG（默认）或 SVG 图片，源码和样式存在形状标记里。编辑和转换会保留位置、缩放和图层顺序。
- 符号库、可视化编辑（MathLive）、符号 / 数字 / 汉字字体、字号、颜色、预设导入导出，都与原版相同。
- **截图识别**：点「截图识别」后直接框选屏幕上的公式，按 Esc 取消；结果填入编辑区，不会自动插入。识别使用与桌面端相同的 MathCraft 公式模型，在本机完成。

原版 Windows 插件的 OLE 公式会被识别出来并原样保留，提示到 Windows 上编辑；Mac 版不能创建 OLE 对象，也没有 MathType 转换。

## 能否复用原项目的插件

原项目的 `office_plugin/` 分为 `windows/`、`macos/` 和 `shared/` 三部分。逐层分析的结论：

| 原项目部分 | 内容 | 结论 |
| --- | --- | --- |
| `windows/` | VSTO 加载项（.NET Framework）、C++ OLE 处理程序、Inno Setup 安装包 | **不能用于本项目**。Office for Mac 没有 VSTO、COM 和自定义 OLE。Windows 用户可以继续单独使用原版 Windows 插件，但它的截图识别要连接原版桌面端的自动化接口，本项目没有这个接口 |
| `shared/EditorAssets`、`shared/mathjax` | 编辑器、任务窗格、符号库、MathLive，MathJax 排版适配器 | **原样复用**，未做修改 |
| `macos/web`、`macos/manifest.xml` | 功能区命令、Word / PowerPoint 的 Office.js 操作、设置和帮助页、加载项清单 | **复用**。这些页面只通过固定的 `/api` 接口和宿主通信，与宿主用什么实现无关。只改了几句提到「桌面端 / 自动化接口」的识别提示文字，见下文 |
| `macos/host` | Python 宿主（PyObjC、lxml，PyInstaller 打包）：HTTPS 服务、证书、安装、CoreText 字形轮廓、OMML、设置、Word 文档变量、自动化接口转发 | **不复用，按本项目重写为 Rust**。原因有二：截图识别要把请求转发给原版桌面端的自动化接口（`next_result`），本项目没有这个接口，在 macOS 上本项目又会替换原版桌面端，识别会直接失效；此外还要为它引入一整套 Python 构建链 |
| `src/assets/MathJax`（office 资源配置） | MathJax 4.1.3 及字体的裁剪子集 | 从 npm 安装同一精确版本，构建时逐个文件比对原项目记录的 SHA-256，结果与原项目的文件逐字节一致 |

重写后的宿主实现了原版的全部 `/api` 接口，请求、返回和错误码都相同，页面不用改动。截图识别不再转发给桌面端，改为宿主自己调用系统的 `screencapture` 框选，再用链接进来的 MathCraft 引擎（`crates/mathcraft`，与桌面端共用的同一份 Rust 代码）识别。这样插件本身就能截图识别，DMG 也比原版小（15 MB，原版不含识别引擎时为 23 MB）。

### 对复用页面的改动

只改了下列文字，代码一行未动：

- `web/app/strings.mjs`：`ConnectedAutomationStatus`、`OcrWaitingStatus`、`OcrEmpty`、`OcrTimeout`、`InvalidResponse`（中英文），去掉「请在桌面端识别」「自动化接口」等提法。
- `web/help.html`：「开始使用」中关于截图识别的提示（中英文）。

复用的文件来自原仓库 `f867c2b` 之后的本地工作区（macOS 插件在该提交时尚未提交到原仓库）。

## 结构

```
office_plugin/
  web/  shared/  support/  manifest.xml   复用的插件页面、编辑器、帮助页图片、清单模板
  mathjax/          MathJax 配置与 office 资源清单（含上游文件的 SHA-256）
  host/             插件宿主 latexsnipper-office-host（Rust）
  app/              设置窗口 LaTeXSnipper Office.app（Tauri），打包宿主和页面
  tools/            assemble.mjs 组装页面资源，build.mjs 构建应用和 DMG
  tests/            原版的页面单元测试（Word 契约、OOXML、LaTeX 解析、PowerPoint 标记）和设置窗口测试
  e2e/              WebKit 中的页面与设置窗口测试
```

| 宿主模块 | 作用 | 对应原版 |
| --- | --- | --- |
| `http.rs`、`api.rs` | 本机 HTTPS（只监听 127.0.0.1），静态文件与 `/api` 路由；`/api` 必须同源并带 `X-LaTeXSnipper-Office` 标记 | `server.py`、`services.py` |
| `cert.rs` | 只对 localhost 有效的非 CA 自签证书，信任与移除 | `certificate.py` |
| `install.rs` | 登录项、插件清单（图标 URL 带内容哈希）、状态、卸载 | `install.py`、`sideload.py`、`lifecycle.py` |
| `omml.rs`、`xml.rs` | 用 Word 的 `mathml2omml.xsl` 和系统 `xsltproc` 转换，再套用字体、字号和颜色 | `omml.py` |
| `outlines.rs` | CoreText 系统字体轮廓，供排版适配器绘制数字和汉字 | `outlines.py` |
| `typography.rs` | 公式样式、各宿主设置、预设导入导出，格式与 Windows 版相同 | `typography.py`、`options.py` |
| `system.rs` | Word 文档变量（AppleScript）、预设文件对话框 | `word_variables.py`、`dialogs.py` |
| `recognition.rs` | 截图识别任务：`screencapture` 框选，MathCraft 识别 | `recognition.py`（原为转发桌面端） |

设置窗口本身不做任何系统修改，只调用宿主的命令行：

```
latexsnipper-office-host --serve | --install | --uninstall | --status | --download-model | --self-test
```

## 构建

需要 macOS（Apple Silicon）、Xcode 命令行工具、Rust（版本由仓库根目录的 `rust-toolchain.toml` 固定）和 Node.js 22。在 `office_plugin/` 中：

```sh
npm ci
npm run build        # 输出 release/LaTeXSnipperOffice_<版本>_macos_arm64.dmg
```

构建依次组装页面资源（核对 MathJax 文件的 SHA-256），以 release 模式构建宿主，由 Tauri 把设置窗口、宿主和页面打包成 `LaTeXSnipper Office.app` 并做 ad-hoc 签名，最后让打包好的宿主用临时证书和临时端口自检一遍（不安装任何东西）。整个过程不使用桌面端的构建产物。版本号在 `package.json`、`host/Cargo.toml`、`app/Cargo.toml` 和 `app/tauri.conf.json` 中，必须一致。

开发时可以不打包，直接用真实服务启动一个临时服务器（随机端口、临时证书，不安装）：

```sh
npm run assemble
cargo run --manifest-path host/Cargo.toml --example dev_server 28799
```

## 测试

```sh
npm test                                     # 原版页面单元测试 + 设置窗口测试（node --test）
cargo test --manifest-path host/Cargo.toml   # 宿主：HTTPS、接口、OMML、字形轮廓、证书、识别任务
npm run test:e2e                             # WebKit 中的页面与设置窗口
```

- 宿主的集成测试走真实的 TLS，使用组装好的真实页面资源（先运行 `npm run assemble`）。截图、AppleScript 和文件对话框用测试替身代替，不会在屏幕上弹出任何东西，也不会控制 Word。
- 安装了 Word 时，OMML 测试使用 Word 自带的真实样式表；有 MathCraft 模型时（桌面端已下载，或设置了 `LATEXSNIPPER_MODELS`），用真实模型识别测试图片。缺少时这些测试会跳过。
- 端到端测试在无头 WebKit（Office for Mac 使用的引擎）中加载原版页面，经宿主走完 MathJax 4 渲染、排版适配器、CoreText 轮廓和 OMML 转换的整条链路；设置窗口的 Tauri 接口由模拟代替。
- 尚未在真实的 Word / PowerPoint 中做自动化测试：Office 插件只能在已安装并信任证书的 Office 中运行，这一步需要手动验证。
