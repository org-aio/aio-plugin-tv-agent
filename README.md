# 电视 Agent

电视 Agent 是一个 AIO Topcoat 全栈插件，也是一个可直接打包安装的 Android TV 应用。用户可以用自然语言说出片名或题材，应用会通过 AI 提取检索意图，再从 TVBox / 影视仓接口找到在线资源并进入播放器。

## 能力

- AIO 工作空间中的插件页面：`tv-agent`
- Topcoat Rust 后端：`/health`、`/aio/describe`、`/api/catalog`、`/api/agent`、`/api/context`、`/api/settings`
- Android TV 页面：遥控器方向键空间导航、Enter 选集/播放、Back 返回、语音点播
- TVBox / 影视仓搜索：读取公开配置，优先选择可直接播放的 HTTPS `.m3u8` 或 `.mp4`
- Android TV 原生播放：HLS 通过 Media3 ExoPlayer 播放，避免电视 WebView 对 `.m3u8` 支持不完整
- 离线兜底：网络或影视源不可用时保留三支开放授权演示短片
- Android WebView 通过 `TvAgentBridge` 提供与 `window.aioPlugin.json()` 相同形状的请求结果

## 配置

本地开发可以复制 `.env.example` 为 `.env.local`，填入 AI 服务配置。`.env.local` 已加入 `.gitignore`，不要提交真实密钥。

```bash
cp .env.example .env.local
```

AIO 安装环境在插件设置页填写 AI Key，密钥会以密文保存到插件专属数据库；接口只会返回是否已经配置。管理员还需要在宿主环境同时批准插件的模型和影视出站地址：

```bash
AIO_PROCESS_ENDPOINTS=https://company-ai.addzero.site/v1
AIO_PROCESS_HTTP_ENDPOINTS=https://szyyds.cn/tv/x.json,https://cj.lziapi.com/api.php/provide/vod,...
```

完整地址列表以 [aio-plugin.toml](aio-plugin.toml) 为准。

## 构建 AIO 插件

仓库使用 AIO 固定的 Rust `nightly-2026-05-25` 与 Topcoat 0.6.2。首次构建前确保存在 `cargo-zigbuild` 和 Zig：

```bash
cargo install cargo-zigbuild --locked
brew install zig
sh scripts/build.sh
aio plugin validate .
aio plugin package . --version 0.1.0
```

最终插件产物位于 `dist/`，其中 `dist/server` 是 Linux x86_64 服务端，`dist/frontend/` 包含完整页面、视频和海报，`dist/android/tv-agent-debug.apk` 是同一次构建的 Android TV 安装包。

## 构建 Android TV APK

Android 工程位于 `android/`，直接复用根目录的 `frontend/`。构建时通过环境变量或 Gradle 参数注入 AI Key：

```bash
export AIO_TV_AGENT_AI_KEY='<AI API Key>'
cd android
./gradlew assembleDebug
cp app/build/outputs/apk/debug/app-debug.apk ../dist/android/tv-agent-debug.apk
```

也可以使用 `-PtvAgentAiKey=<AI API Key>`。Key 会进入 BuildConfig，不会写入前端 JavaScript；发布 APK 前仍应确认目标分发环境对本地构建配置的访问控制。

安装到已连接的电视或模拟器：

```bash
adb install -r ../dist/android/tv-agent-debug.apk
```

应用同时注册 `LAUNCHER` 与 `LEANBACK_LAUNCHER`，支持没有触摸屏的电视设备。WebView 页面加载本地资源，点播 `.m3u8` 或 `.mp4` 时由 `TvAgentBridge` 打开 Media3 ExoPlayer 原生播放器；AIO 插件模式则由宿主注入 `window.aioPlugin`，页面无需维护两套业务协议。

## 本地预览

不要直接双击 `frontend/index.html`。浏览器以 `file://` 打开时没有 AIO 宿主或 Android 桥，也无法访问 `/api`。使用仓库内的开发脚本启动同源服务：

```bash
sh scripts/dev.sh
```

然后打开 `http://127.0.0.1:3000/`。该进程由同一个 Topcoat Router 提供静态前端和真实的 `/api/catalog`、`/api/agent`、`/api/context` 接口；AIO 打包运行时仍由宿主负责注入前端。开发脚本会在存在 `.env.local` 时自动加载，但固定监听 `127.0.0.1`，避免使用机器全局 `HOST`。

## 接口

`GET /api/catalog?q=动物` 返回推荐与目录。`POST /api/agent` 接收 `{"message":"我想看斗破苍穹"}`，返回 `intent`、`message`、`suggestions` 和可选的 `selection`。当 `intent` 为 `play` 时，前端直接播放所选剧集。

影视数据来自用户指定的第三方 TVBox 配置和采集站。项目不托管、不复制影视内容，也不保证第三方源的可用性、稳定性和内容授权。

第三方视频素材授权见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
