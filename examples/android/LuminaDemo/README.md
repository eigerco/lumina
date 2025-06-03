# Instructions to re-create the setup

- Create a new kotlin project in xcode
- make sure you have all the default android targets installed
```
rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android
```

...

- build the Android artifacts `./node-uniffi/build-android.sh`
- copy the bindings into the project
```
cp -rv node-uniffi/app examples/android/LuminaDemo
```

- add jna import to `build.gradle.kt`, required to load natively compiled code
```
implementation("net.java.dev.jna:jna:5.7.0@aar")
```
