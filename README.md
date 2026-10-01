# 电视 Agent

电视 Agent 是一个 AIO Topcoat 全栈插件，也是一个可直接打包安装的 Android TV 应用。用户可以用自然语言说出片名或题材，应用会通过 AI 提取检索意图，再从 TVBox / 影视仓接口找到在线资源并进入播放器。

## 能力

- AIO 工作空间中的插件页面：`tv-agent`
- Topcoat Rust 后端：`/health`、`/aio/describe`、`/api/catalog`、`/api/browse`、`/api/agent`、`/api/context`、`/api/settings`、`/api/models`、`/api/models/test`、`/api/danmaku`
- Android TV 页面：遥控器方向键空间导航、Enter 选集/播放、Back 返回、语音点播
- 电视大屏首页：16:9 封面货架，聚合电影、动漫、电视剧、综艺、短剧和直播；可以点“继续翻”持续浏览，不需要逐个选择影视源
- TVBox / 影视仓搜索：读取公开配置，优先选择可直接播放的 HTTPS `.m3u8` 或 `.mp4`
- Android TV 原生播放：HLS 通过 Media3 ExoPlayer 播放，避免电视 WebView 对 `.m3u8` 支持不完整
- 网页播放兼容：Chrome 等不支持原生 HLS 的浏览器自动使用内置 hls.js，支持直播和点播 M3U8
- TV 播放控制：前进/后退 15 秒、0.5x 到 2.0x 倍速、弹幕开关和全屏切换；Back 键按控制层、全屏层、退出播放器逐级处理
- 离线兜底：网络或影视源不可用时保留三支开放授权演示短片
- Android WebView 通过 `TvAgentBridge` 提供与 `window.aioPlugin.json()` 相同形状的请求结果

## 配置

本地开发可以复制 `.env.example` 为 `.env.local`，填入 AI 服务配置。`.env.local` 已加入 `.gitignore`，不要提交真实密钥。

```bash
cp .env.example .env.local
```

AIO 安装环境在插件设置页配置模型 API 地址、模型名和 AI Key。模型地址与模型名按租户和用户保存，密钥会以密文保存到插件专属数据库；设置接口只返回地址、模型名和 `has_secret`，不会回传明文密钥。切换模型 API 地址时必须重新输入 Key，避免把旧服务的凭据发送到新地址。

设置页支持读取 OpenAI 兼容的 `/models` 列表，并会用一次最小 Chat Completions 请求实际验证模型推理连通性。模型配置和 TVBox 片源列表都按租户和用户保存；搜索时会读取当前用户保存的片源配置，不需要重启插件。

弹幕接口为可选配置，遵循 TVBox / FongMi 协议。AIO 安装模式必须使用带 `{name}` 或 `{episode}` 的 HTTPS GET 模板，例如 `https://example.com/search?name={name}&episode={episode}`，并把完整接口加入宿主 `http_endpoints`；接口返回的弹幕索引 URL 也必须属于同一份授权列表。Android 本地模式同时支持模板 GET 和 form POST。没有配置接口时播放不受影响，播放器会提示未配置弹幕。

管理员需要在宿主环境同时批准插件的模型和影视出站地址：

```bash
AIO_PROCESS_ENDPOINTS=https://company-ai.addzero.site/v1
AIO_PROCESS_HTTP_ENDPOINTS=https://raw.githubusercontent.com/TVboxorg/TVbox/main/dist/official.json,https://raw.githubusercontent.com/wangguo0/tvbox-sub/main/merged.json,...
```

完整地址列表以 [aio-plugin.toml](aio-plugin.toml) 为准。弹幕接口不是默认服务，需由部署管理员按实际接口单独授权。

AIO 宿主的 `http_endpoints` 是对完整 URL 的精确授权，且单个 broker 响应上限为 512 KB。`TVboxorg/TVbox`、`wangguo0/tvbox-sub` 和 `ZHOUYU86/tvbox` 可以作为 AIO 配置入口；`hebijunge/tvbox-config` 与 `haygcao/tvbox-master-aggregator` 的远端配置超过上限，只在 Android 本地模式启用。AIO 安装模式和 Android 本地模式都会并发搜索已授权的采集站，并按源列表顺序返回首个有效结果。

## TVBox 上游

默认配置参考以下公开仓库：

- `TVboxorg/TVbox`：官方示例配置，AIO 可用入口
- `wangguo0/tvbox-sub`：合并订阅，AIO 可用入口
- `ZHOUYU86/tvbox`：精简配置，AIO 可用入口
- `hebijunge/tvbox-config`：大型聚合配置，Android 可用，AIO 响应可能超限
- `haygcao/tvbox-master-aggregator`：大型聚合配置，Android 可用，AIO 响应可能超限
- `rzhnrhjr6j-cloud/tvbox-source-monitor`：多仓监控索引，用于发现配置，不默认内置
- `raul1584/tvbox-premium`：Android TVBox 客户端，不包含本插件配置
- `cluntop/tvbox`：TVBox / OK 影视接口说明，不包含本插件配置

这些仓库只提供配置或客户端说明，不包含本项目托管的影视内容。默认启用的采集站来自配置中的公开 CMS 接口；实际内容授权和稳定性由相应站点负责。

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

Android 工程位于 `android/`，直接复用根目录的 `frontend/`。APK 不内嵌 AI Key，安装后在应用的“模型设置”页填写；Key 由 Android Keystore 加密后保存在应用私有存储中。

```bash
cd android
./gradlew assembleDebug
cp app/build/outputs/apk/debug/app-debug.apk ../dist/android/tv-agent-debug.apk
```

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

`GET /api/catalog?q=动物` 返回自然语言搜索或演示目录；`GET /api/browse?category=movie&page=1&page_size=24` 返回聚合片库，`category` 支持 `all`、`movie`、`anime`、`series`、`variety`、`short`、`live`，每部内容带 `content_type` 和可直接播放的剧集地址。`GET /api/settings` 返回当前模型配置元数据，`POST /api/settings` 保存配置，`POST /api/models` 读取模型列表，`POST /api/models/test` 执行真实推理测试。`POST /api/danmaku` 接收 `{"title":"斗破苍穹","episode":"第 1 集"}` 并返回弹幕时间轴。`POST /api/agent` 接收 `{"message":"我想看斗破苍穹"}`，返回 `intent`、`message`、`suggestions` 和可选的 `selection`。当 `intent` 为 `play` 时，前端显示选集面板，选择剧集后播放。

片库采用两层缓存：浏览器与 Android WebView 会把最近浏览的分类页写入本地存储，打开首页时立即恢复已有卡片，再后台更新；服务端会缓存 TVBox 来源列表和分类页，缓存未过期时直接返回，过期后先返回旧页并在后台刷新，上游暂时失败时继续回退到旧页。缓存按影视源配置、分类、页码和每页数量隔离，修改片源配置后不会复用旧来源结果。

影视数据来自用户指定的第三方 TVBox 配置和采集站。项目不托管、不复制影视内容，也不保证第三方源的可用性、稳定性和内容授权。

第三方视频素材授权见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
