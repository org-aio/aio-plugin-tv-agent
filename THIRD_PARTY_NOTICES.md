# 第三方素材说明

应用内的演示视频与海报来自以下开放影视素材，仅用于展示点播和播放流程。

## Big Buck Bunny

- 文件：`frontend/assets/videos/forest-run.mp4`
- 来源：Blender Foundation
- 授权：Creative Commons Attribution 3.0
- 说明：视频经过转码和裁切，用作《森林狂奔》演示剧集。

## Caminandes: Llama Drama

- 文件：`frontend/assets/videos/llama-drama.mp4`
- 来源：Blender Foundation
- 授权：Creative Commons Attribution 3.0
- 说明：视频经过转码和裁切，用作《雪原小羊驼》演示剧集。

## Coffee Run

- 文件：`frontend/assets/videos/coffee-run.mp4`
- 来源：Blender Foundation
- 来源页面：<https://commons.wikimedia.org/wiki/File:Coffee_Run_-_Blender_Open_Movie-full_movie.webm>
- 授权：Creative Commons Attribution 3.0
- 说明：视频经过转码和裁切，用作《咖啡奇旅》演示剧集。

海报为上述视频的静止画面裁切，授权与对应源视频一致。

## hls.js

- 文件：`frontend/vendor/hls.min.js`
- 版本：1.7.3
- 来源：<https://github.com/video-dev/hls.js>
- 授权：Apache License 2.0
- 说明：用于在网页播放器与 Android WebView 中播放 M3U8/HLS 视频。完整许可证见 `frontend/vendor/hls.js.LICENSE`。

## TVBox 配置与采集接口

应用默认参考 `TVboxorg/TVbox`、`hebijunge/tvbox-config`、`haygcao/tvbox-master-aggregator`、`wangguo0/tvbox-sub` 和 `ZHOUYU86/tvbox` 的公开配置，并使用其中的公开 CMS 接口进行搜索。`rzhnrhjr6j-cloud/tvbox-source-monitor` 提供多仓监控索引；`raul1584/tvbox-premium` 和 `cluntop/tvbox` 是客户端或接口说明参考，不作为默认片源。第三方站点的片源内容、可用性、授权和变更不由本项目控制。
