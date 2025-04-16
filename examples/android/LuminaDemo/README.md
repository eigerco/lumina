# Instructions to re-create the setup

- Create a new kotlin project in xcode
- make sure you have aarch64-apple-ios target installed + simulator
```
rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android thumbv7em-none-eabihf wasm32-unknown-unknown x86_64-linux-android
```

...

- build the Android artifacts `./node-uniffi/build-android.sh`
- `cp -rv node-uniffi/app examples/android/LuminaDemo`
- add imports to `build.gradle.kt`
```
implementation("net.java.dev.jna:jna:5.7.0@aar")
```


