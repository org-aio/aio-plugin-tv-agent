# 电视 Agent

电视 Agent 是一个 AIO Topcoat 全栈插件，也是一个可直接打包安装的 Android TV 应用。用户可以用自然语言说出题材、心情或片名，应用会调用 Agent 接口选择短剧并进入播放器。

## 能力

- AIO 工作空间中的插件页面：`tv-agent`
- Topcoat Rust 后端：`/health`、`/aio/describe`、`/api/catalog`、`/api/agent`、`/api/context`
- Android TV 页面：遥控器方向键空间导航、Enter 播放、Back 返回、语音点播
- 离线演示目录与视频：三支开放授权短片作为短剧内容
- Android WebView 通过 `TvAgentBridge` 提供与 `window.aioPlugin.json()` 相同形状的请求结果

## 构建 AIO 插件

仓库使用 AIO 固定的 Rust `nightly-2026-05-25` 与 Topcoat 0.6.2。首次构建前确保存在 `cargo-zigbuild` 和 Zig：

```bash
cargo install cargo-zigbuild --locked
brew install zig
sh scripts/build.sh
aio plugin validate .
aio plugin package . --version 0.1.0
```

最终插件产物位于 `dist/`，其中 `dist/server` 是 Linux x86_64 服务端，`dist/frontend/` 包含完整页面、视频和海报。

## 构建 Android TV APK

Android 工程位于 `android/`，直接复用根目录的 `frontend/`：

```bash
cd android
./gradlew assembleDebug
cp app/build/outputs/apk/debug/app-debug.apk ../dist/android/tv-agent-debug.apk
```

安装到已连接的电视或模拟器：

```bash
adb install -r ../dist/android/tv-agent-debug.apk
```

应用同时注册 `LAUNCHER` 与 `LEANBACK_LAUNCHER`，支持没有触摸屏的电视设备。WebView 页面加载本地资源；AIO 插件模式则由宿主注入 `window.aioPlugin`，页面无需维护两套业务协议。

## 接口

`GET /api/catalog?q=动物` 返回推荐与目录。`POST /api/agent` 接收 `{"message":"来一部治愈的动物短剧"}`，返回 `intent`、`message`、`suggestions` 和可选的 `selection`。当 `intent` 为 `play` 时，前端直接播放所选剧集。

当前目录是可替换的离线演示内容。接入真实短剧源时，只需替换服务端目录数据和视频地址，不需要改变前端交互协议。

第三方视频素材授权见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
