<center>

## euv-app

<img src="https://ltpp.vip/github/pages/docs-pages/pages/img/euv.png" alt="" height="160">

[![](https://github.com/euv-dev/euv-app/workflows/Build%20APK/badge.svg)](https://github.com/euv-dev/euv-app/actions?query=workflow:Build+APK)
[![](https://img.shields.io/github/release/euv-dev/euv-app/latest.svg)](https://github.com/euv-dev/euv-app/releases/latest)
[![](https://img.shields.io/github/license/euv-dev/euv-app.svg)](https://github.com/euv-dev/euv-app/blob/master/LICENSE)

</center>

[Download APK](https://github.com/euv-dev/euv-app/releases/latest/download/euv.apk)

> Android client for the **euv** UI framework — bundles the euv web frontend as a standalone APK via Tauri 2.x with a Rust bridge for native operations.

## What it is

`euv-app` packages [euv](https://github.com/euv-dev/euv) as an installable Android app. It is a Tauri 2.x shell (`productName=Euv`, `identifier=com.euv`) that:

- Serves the euv web frontend from a custom-protocol cache layer (`euv.localhost`)
- Exposes a Rust IPC bridge (`src-tauri/src/bridge/`) for native operations (external URLs, logging)
- Ships a pre-fetched `bundled-cache/` so the app can run offline
- Runs edge-to-edge immersive — the page renders under the Android status bar via the `__EUV_IMMERSIVE__` injection (see [Immersive safe-area](src-tauri/src/cache/fn.rs))

## Build

```sh
# install JS deps
npm install

# release Android APK
npm run build:android

# debug Android APK
npm run build:android:debug

# regenerate icons
npm run icons
```

The full fresh-clone bootstrap is in [`build.sh`](build.sh) — it self-installs rustup Android targets, JDK, and the Android SDK/NDK into `sdk/` via `scripts/setup-sdk.sh`, runs the page prefetch (`prefetch-cache.js && apply-config.js`), and invokes `tauri android build`. A signed `euv.apk` is the output artifact.

Every push to `master` builds the APK via the [Build APK workflow](.github/workflows/build-apk.yml) and publishes it to the rolling [latest](https://github.com/euv-dev/euv-app/releases/tag/latest) GitHub Release.

## Project layout

```
euv-app/
├── src-tauri/          # Tauri 2.x Rust crate (cdylib + rlib, name=euv-app, lib=euv_lib)
│   ├── src/
│   │   ├── main.rs     # Tauri entrypoint
│   │   ├── lib.rs      # euv_lib root — exports `run()`
│   │   ├── bridge/     # Web ↔ Rust IPC bridge
│   │   ├── cache/      # bundled-cache layer + immersive script injection
│   │   └── log/        # tauri-plugin-log wrapper + macros
│   ├── capabilities/   # Tauri 2.x permissions
│   └── gen/android/    # Tauri-generated JNI scaffolding
├── scripts/
│   ├── apply-config.js       # patches app.config.json into dist/
│   ├── prefetch-cache.js     # warms bundled-cache for offline use
│   ├── gen-icons.py          # regenerate app icons
│   └── setup-sdk.sh          # install Android SDK / NDK
├── sdk/                # Android SDK + NDK (locally installed, untracked)
├── bundled-cache/      # pre-fetched asset cache
└── dist/               # built frontend
```

## License

This project is licensed under the MIT License, inherited from the upstream [euv framework](https://github.com/euv-dev/euv/blob/master/LICENSE).

## Contributing

Contributions are welcome! Please open an issue or submit a pull request.

## Contact

For any inquiries, please reach out to the author at [root@ltpp.vip](mailto:root@ltpp.vip).