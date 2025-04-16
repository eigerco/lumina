# Instructions to re-create the setup

- Create a new kotlin project in xcode
- make sure you have aarch64-apple-ios target installed + simulator
```
rustup target add armv7-linux-androideabi
```

...

- build the Android artifacts `./node-uniffi/build-android.sh`
- `cp -rv node-uniffi/app examples/android/`
- add imports to `build.gradle.kt`
```
implementation("net.java.dev.jna:jna:5.7.0@aar")
```


